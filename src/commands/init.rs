//! `chap-devops init` — write a deployment directory.

mod adopt;
mod chap_core;
mod components;
mod dropped;
mod env;
mod inputs;
mod ports;
mod summary;
mod takeover;

use crate::cli::InitArgs;
use crate::commands::Ctx;
use crate::components::{COMPONENTS_FILE, Component};
use crate::compose::spec::EnvSpec;
use crate::compose::sync::{write_dhis2_config, write_ocs_config};
use crate::compose::{API_SERVICE, apply, render_env};
use crate::error::{ChapError, Result};
use crate::project::{
    API_PORT_ENV_VAR, CHAP_DEVOPS_DIR, ComposeSource, DEFAULT_PORT_RANGE, ENV_FILE, MODELS_FILE,
    PROJECT_FILE, Project, ProjectState,
};
use crate::registry;
use chap_core::{CHECKOUT_TAG, ChapCore, checkout_source, resolve_chap_core};
use components::{ComponentFlags, parse_component_list, parse_components};
use dropped::{
    component_dropped, dropped_components, remove_dropped_component_files, remove_stale_overlays,
    stop_dropped,
};
use env::{
    EnvAction, KeptTag, env_action, env_api_port, env_auth, move_kept_tag, random_password,
    resolve_secrets,
};
use inputs::{carried_manual, compose_project, parse_models, resolve_dir};
use ports::{own_running_claims, warn_about_ports};
use summary::summary;
use takeover::{adopted_secrets, adoption_lines, finish_adoption, start_adoption};

/// The `--chap-tag` value that means "whatever the newest release is".
pub const MOVING_DEFAULT_TAG: &str = "latest";
/// Database user and database name of the generated `.env`.
const POSTGRES_USER: &str = "chap";
const POSTGRES_DB: &str = "chap_core";

/// Create compose.yml, compose.marketplace.yml, the model overlays, .env and
/// the `.chap-devops/` directory in the target directory.
///
/// `args.source` names a chap-core checkout to build from instead of a
/// release to pull; see [`checkout_source`].
/// Read the DHIS2 version of a local seed dump and settle the tag with it:
/// the tag of the dump when `--dhis2-tag` is not given, and a refusal for a
/// tag older than the dump or a dump older than 2.41. The dump is read where
/// the bind mount finds it, in the deployment directory. A dump that is not
/// there yet is not read, and the note says so. A URL is checked by the dump
/// step of the first `chap-devops up`.
fn settle_dump_version(
    components: &mut crate::components::Components,
    dir: &std::path::Path,
    tag: Option<&str>,
) -> Result<Option<String>> {
    let crate::components::Dhis2Seed::From(source) = &components.dhis2.seed else {
        return Ok(None);
    };
    if !components.dhis2.enabled || components.dhis2.seed.is_url() {
        return Ok(None);
    }
    // Only the file the bind mount uses: a path is relative to the deployment
    // directory, and a file of the same name in the working directory is not
    // the one the restore reads.
    let found = dir.join(source);
    if !found.is_file() {
        return Ok(Some(format!(
            "{source} is not in {}, so chap-devops did not read its DHIS2 version, and DHIS2 runs {}",
            dir.display(),
            components.dhis2.image_tag
        )));
    }
    crate::output::notice(&format!(
        "reading the DHIS2 version of {source} from its Flyway table; a large dump takes \
         about a minute"
    ));
    let Some(migration) = crate::components::dump_migration(&found)? else {
        return Ok(None);
    };
    let Some(minor) = crate::components::migration_minor(&migration) else {
        return Ok(None);
    };
    components.dhis2.image_tag = crate::components::tag_for_dump(&minor, tag)?;
    Ok(Some(match tag {
        Some(_) => format!("{source} is DHIS2 {minor} (Flyway migration {migration})"),
        None => format!(
            "{source} is DHIS2 {minor} (Flyway migration {migration}), so DHIS2 runs {minor}"
        ),
    }))
}

pub fn run(ctx: &Ctx, args: &InitArgs) -> Result<()> {
    create(ctx, args, true)
}

/// [`run`], printing its report only when `report_it` is set: `chap-devops run`
/// creates its deployment on the way to something else, and says so itself.
pub(crate) fn create(ctx: &Ctx, args: &InitArgs, report_it: bool) -> Result<()> {
    let checkout = args.source.as_deref().map(checkout_source).transpose()?;
    // The positional DIR is relative to the working directory, not to -C:
    // `chap-devops init foo` is a fresh deployment, not an operation on a project.
    let dir = resolve_dir(&args.dir)?;
    if Project::exists(&dir) && !args.force {
        return Err(ChapError::AlreadyInitialized(dir).into());
    }
    // Nesting is allowed (it is the user's directory), but commands run from
    // in here will find this project, not the outer one, so say so.
    if let Some(parent) = dir.parent().and_then(Project::find_root) {
        crate::output::warn(&format!(
            "{} is inside the chap-devops deployment at {}; commands run below it will use the new deployment",
            dir.display(),
            parent.display()
        ));
    }

    let mut registry = registry::load(&ctx.registry)?;
    // A takeover reads the old deployment, and refuses, before anything is
    // decided: its project name, secrets and models are what this run writes.
    let adoption = match &args.adopt {
        Some(old) => Some(start_adoption(ctx, args, &dir, old, &registry)?),
        None => None,
    };
    // chap-core's compose files publish the API on 8000, and what calls it
    // (a DHIS2 route, a script) calls it there; a takeover keeps the port
    // unless --api-port names another.
    let kept_port;
    let args = match &adoption {
        Some(_) if args.api_port == crate::project::DEFAULT_API_PORT => {
            kept_port = InitArgs {
                api_port: adopt::ADMIN_API_PORT,
                ..args.clone()
            };
            &kept_port
        }
        _ => args,
    };
    // What the deployment is made of is settled first: it decides which
    // compose files are rendered at all, and an unknown name in --with is a
    // typo to report before anything is written.
    let mut components = parse_components(&ComponentFlags::from_args(args, ctx.registry.offline))?;
    if let Some(note) = settle_dump_version(&mut components, &dir, args.dhis2_tag.as_deref())? {
        crate::output::notice(&note);
    }
    // A chap-core elsewhere is instead of this deployment's own, never beside it.
    if let Some(url) = &args.chap_core_url {
        components.set_enabled(Component::ChapCore, false);
        let mut external = crate::components::external_chap_core(url)?;
        if let Some(note) = crate::components::detect_models_host(
            &mut external,
            &crate::docker::container_publishing,
        ) {
            crate::output::warn(&note);
        }
        components.chap_core_external = Some(external);
    }
    // The deployment this run is writing over, when there is one: `--force`
    // starts the state over, so anything it had and this run does not ask for is
    // going, and the operator hears which before the directory is rewritten.
    let previous = Project::load(&dir).ok();
    let dropped = match &previous {
        Some(previous) => dropped_components(
            &previous.state.components,
            &components,
            &parse_component_list(args.without.as_deref()).unwrap_or_default(),
        ),
        None => Vec::new(),
    };
    for component in &dropped {
        crate::output::warn(&component_dropped(*component));
    }
    // A re-init resets the component set from the flags and the defaults, so the
    // DHIS2 pin it records can move while the database the previous one created
    // is still on this machine. DHIS2 migrates a schema forward only, so that is
    // said before anything is written rather than found out at `chap-devops up`.
    if let Some(previous) = &previous
        && let Some(note) = crate::commands::components::dhis2_tag_moved(previous, &components)
    {
        crate::output::warn(&note);
    }
    // Settle the chap-core tag and the compose file that goes with it before
    // anything is written: both need the network, and a failure of either is a
    // warning plus a fallback, never a half-written directory.
    // A deployment without chap-core asks GitHub nothing: the tag is recorded
    // as given and `compose.yml` would come from the embedded copy, which is
    // what `chap-devops components enable chap-core` renders and `chap-devops update`
    // then moves to a release.
    if checkout.is_some() && !components.chap_core.enabled {
        return Err(anyhow::anyhow!(
            "--source builds chap-core, and this run leaves chap-core out; \
             drop --source, or keep chap-core in the component set"
        ));
    }
    let chap_core = if let Some(path) = &checkout {
        // Nothing to look up: the checkout is the version.
        ChapCore {
            tag: CHECKOUT_TAG.to_string(),
            source: ComposeSource::Checkout { path: path.clone() },
            cached: None,
        }
    } else if components.chap_core.enabled {
        // A takeover keeps the chap-core the old `.env` pinned, unless
        // --chap-tag names another.
        let tag = match adoption.as_ref().and_then(|a| a.chap_tag.as_deref()) {
            Some(old) if args.chap_tag == MOVING_DEFAULT_TAG => old,
            _ => args.chap_tag.as_str(),
        };
        resolve_chap_core(ctx, &dir, tag)
    } else {
        ChapCore {
            tag: args.chap_tag.trim().to_string(),
            source: ComposeSource::Embedded,
            cached: None,
        }
    };
    let state = ProjectState {
        chap_image_tag: chap_core.tag.clone(),
        chap_compose_source: chap_core.source.clone(),
        compose_project: match &adoption {
            Some(adoption) => adoption.project.clone(),
            None => compose_project(&dir)?,
        },
        registry_url: ctx.registry.url.clone(),
        api_port: args.api_port,
        port_range: (args.port_base, DEFAULT_PORT_RANGE.1.max(args.port_base)),
        components: components.clone(),
        // A re-init keeps the deployment's own model definitions: `--force`
        // rewrites the directory, and dropping them would leave `--models`
        // naming models nothing has heard of.
        manual: carried_manual(&dir),
        ..ProjectState::default()
    };
    // The ports the deployment publishes, and a taken one is only a problem at
    // `chap-devops up`: the process holding one may well be a previous stack this
    // deployment is meant to replace, and a deployment that is down is not
    // holding anything yet. So: a warning with a way out, not a refusal to
    // write the directory.
    let others = crate::ports::other_deployments(&dir, &crate::docker::compose_ls_json);
    let own = previous
        .as_ref()
        .map(own_running_claims)
        .unwrap_or_default();
    let ports = warn_about_ports(
        &components,
        args.api_port,
        &crate::ports::is_busy,
        &others,
        &own,
    );
    let api_port_busy = ports.busy.iter().any(|c| c.service == API_SERVICE);
    let mut project = Project {
        dir: dir.clone(),
        state,
    };
    // The carried-over definitions are part of the catalogue from here on,
    // so `--models` and the browser see them like any marketplace entry.
    for warning in registry.with_manual(&project.state.manual) {
        crate::output::warn(&warning);
    }

    // Decide what to enable before writing anything, so a cancelled browser
    // or a typo in --models leaves no half-written directory behind.
    let selection = if let Some(adoption) = &adoption {
        adoption.selection()
    } else if args.interactive {
        // Quitting the browser without saving is "no models", not an error.
        crate::tui::run_tui(ctx, &project, &registry)?.unwrap_or_default()
    } else {
        parse_models(&args.models, &registry)?
    };
    // The whole selection is checked against the registry here, before a file
    // is deleted or written: a starter id, a model the marketplace does not
    // list or a version that was yanked must leave an existing deployment
    // exactly as it was, rather than taking its overlays with it on the way
    // out. apply() checks again; this is the run that has something to lose.
    crate::compose::apply::validate(&project, &registry, &selection)?;

    // `.env` is operator-owned state - the database password, the API token,
    // the registration key, the image pins - so `init` decides its fate before
    // it writes anything. Rewriting it under a postgres volume that still
    // holds the old role password leaves chap-core looping on a failed
    // authentication, which is why even `--force` keeps the file.
    let env_path = dir.join(ENV_FILE);
    let env_exists = env_path.is_file();
    let env = env_action(env_exists, args.fresh_env, args.no_env);
    // The warnings about `.env` belong to the result: they close the report.
    let mut warnings = Vec::new();
    // Compose reads `.env` after the compose files, so a CHAP_API_PORT line in
    // a file this run is keeping wins over `--api-port`. Say so rather than
    // leaving the API on a port nothing in `.chap-devops/` mentions.
    if env == EnvAction::Kept
        && let Ok(body) = std::fs::read_to_string(&env_path)
        && let Some(pinned) = env_api_port(&body).filter(|p| *p != args.api_port)
    {
        warnings.push(format!(
            ".env already sets {API_PORT_ENV_VAR}={pinned}, and compose reads that after the \
             compose files, so the API stays on {pinned} rather than {}; edit that line to \
             move it",
            args.api_port
        ));
    }

    // `--api-token` only means something for a `.env` this run writes: the
    // secrets have to be in the file compose reads, and a kept file is the
    // operator's. It is checked here, before any container is stopped or any
    // file removed, so a rejected token leaves an existing deployment as it
    // was.
    let secrets = match env {
        EnvAction::Written if adoption.is_some() => {
            adopted_secrets(adoption.as_ref().map(|a| &a.env))?
        }
        EnvAction::Written => resolve_secrets(args.api_token.as_ref())?,
        _ => {
            if args.api_token.is_some() {
                warnings.push(format!(
                    "--api-token needs a .env to write to, and this run {}; \
                     run `chap-devops auth enable` in the deployment instead",
                    match env {
                        EnvAction::Kept => "is keeping the one already there",
                        _ => "writes none (--no-env)",
                    }
                ));
            }
            None
        }
    };

    // The containers of everything this run takes away go first, while the
    // compose files that define them are still on disk: a service whose
    // definition has been removed cannot be stopped by name any more, and one
    // left running keeps its host port published long after the deployment
    // stopped asking for it. Read from the previous project, because that is the
    // one whose `-f` list still names those files.
    let mut dropped_notes = Vec::new();
    if let Some(previous) = &previous {
        dropped_notes.extend(stop_dropped(previous, &components, &selection));
    }

    // --force starts the state over, so overlays the previous project owned
    // would otherwise linger: unreferenced by the umbrella, but still holding
    // their host port against the allocator. The compose file of a component
    // that is not coming back is the same problem, and worse: nothing in
    // `.chap-devops/` would mention it afterwards, so no later `chap-devops sync` would ever
    // remove it either.
    let mut stale = remove_stale_overlays(&dir, &selection);
    if let Some(previous) = &previous {
        stale.extend(remove_dropped_component_files(
            &dir,
            &previous.state.components,
            &components,
        ));
    }

    std::fs::create_dir_all(&dir)
        .map_err(|e| anyhow::anyhow!("creating {}: {e}", dir.display()))?;
    // The state lock every other writer of `.chap-devops/` takes: an `init --force`
    // over a deployment another chap-devops is changing waits for it rather than
    // writing over it half-way.
    std::fs::create_dir_all(dir.join(CHAP_DEVOPS_DIR))
        .map_err(|e| anyhow::anyhow!("creating {}: {e}", dir.join(CHAP_DEVOPS_DIR).display()))?;
    let _lock = crate::project::StateLock::acquire(&dir)?;
    let mut written = Vec::new();

    // The raw copy of chap-core's compose.ghcr.yml goes in first: `sync`
    // renders compose.yml from it a moment later, and every later sync
    // re-renders from this copy rather than from the network.
    if let Some(body) = &chap_core.cached {
        let chap_devops = dir.join(CHAP_DEVOPS_DIR);
        std::fs::create_dir_all(&chap_devops)
            .map_err(|e| anyhow::anyhow!("creating {}: {e}", chap_devops.display()))?;
        let name = chap_core
            .source
            .cached_file()
            .expect("a downloaded copy has a file name");
        let path = chap_devops.join(name);
        std::fs::write(&path, body)
            .map_err(|e| anyhow::anyhow!("writing {}: {e}", path.display()))?;
        written.push(path);
    }

    if env == EnvAction::Written {
        if env_exists {
            crate::output::warn(
                "--fresh-env rewrote .env with a new POSTGRES_PASSWORD; a database volume \
                 from an earlier `chap-devops up` still holds the old one - drop it with \
                 `chap-devops down --volumes` or change the role with ALTER USER",
            );
        }
        crate::dotenv::write(
            &env_path,
            &render_env(&EnvSpec {
                // A takeover keeps the credentials the database was made with.
                postgres_user: match &adoption {
                    Some(a) => a.env.postgres_user.clone(),
                    None => POSTGRES_USER.to_string(),
                },
                postgres_password: match &adoption {
                    Some(a) => a.env.postgres_password.clone(),
                    None => random_password()?,
                },
                postgres_db: match &adoption {
                    Some(a) => a.env.postgres_db.clone(),
                    None => POSTGRES_DB.to_string(),
                },
                chap_image_tag: Some(chap_core.tag.clone()),
                api_port: args.api_port,
                api_token: secrets.as_ref().map(|s| s.api_token.clone()),
                registration_key: secrets.as_ref().map(|s| s.registration_key.clone()),
                // apply() appends one commented pin per enabled model.
                model_tag_pins: Vec::new(),
            }),
        )?;
        written.push(env_path.clone());
    }
    // A kept `.env` keeps its secrets, but its image pin follows the tag this
    // run records, or compose keeps pulling the previous one.
    let mut tag_moved = None;
    if env == EnvAction::Kept {
        let previous_tag = previous.as_ref().map(|p| p.state.chap_image_tag.as_str());
        match move_kept_tag(&dir, previous_tag, &chap_core.tag)? {
            KeptTag::Same => {}
            KeptTag::Moved { from, to } => tag_moved = Some((from, to)),
            KeptTag::Warning(text) => warnings.push(text),
        }
    }

    // `.env` decides: a kept file that already carries the secrets keeps the
    // deployment authenticated, and a rendered one says so itself. The state
    // file only records which of the two are in use, never their values, and
    // `sync` reads it to decide whether each overlay carries the registration
    // key - so it has to be settled before apply().
    project.state.auth = env_auth(env, &env_path);

    // The OCS instance config goes in before the sync inside apply(), so the
    // `--ocs-*` values land in it rather than the example ones sync falls
    // back to. Like `.env`, an existing file is kept: it is the operator's.
    if components.ocs.enabled {
        if let Some(path) = write_ocs_config(
            &dir,
            &crate::commands::components::request(&args.ocs).into_spec(),
        )? {
            written.push(path);
        }
        // And `--ocs-read-only` after it, on the file this run just scaffolded
        // or on the one that was already there: the scaffold does not carry the
        // key, and one function owns that edit. A file that already says so is
        // left untouched, so it is not reported as written either.
        if components.ocs.read_only
            && let Some(edit) = crate::compose::sync::set_read_only(&dir, true)?
            && edit != crate::compose::sync::KeyEdit::Unchanged
        {
            written.push(project.ocs_config_path());
        }
    }

    // DHIS2's instance config goes in on the same terms, and before apply() for
    // the same reason: `dhis.conf` is mandatory - DHIS2 does not start without it
    // - and it is the operator's from the moment it exists.
    if components.dhis2.enabled
        && let Some(path) =
            write_dhis2_config(&dir, &crate::compose::spec::Dhis2ConfigSpec::default())?
    {
        written.push(path);
    }

    // apply() writes the overlays, compose.marketplace.yml (even with no
    // models) and .chap-devops/.
    let endpoints = crate::manual::Endpoints::from_env(ctx.registry.offline);
    let mut report = apply(&mut project, &registry, &selection, &endpoints)?;
    report.removed.extend(stale);
    let adopted = adoption
        .as_ref()
        .map(|adoption| finish_adoption(ctx, &mut project, adoption));
    written.extend(report.written.iter().cloned());
    written.push(dir.join(CHAP_DEVOPS_DIR).join(PROJECT_FILE));
    written.push(dir.join(CHAP_DEVOPS_DIR).join(MODELS_FILE));
    written.push(dir.join(CHAP_DEVOPS_DIR).join(COMPONENTS_FILE));
    // apply() reports .env again when it appends a pin to the file init just
    // wrote; the summary lists each file once.
    let mut seen = std::collections::BTreeSet::new();
    written.retain(|path| seen.insert(path.clone()));
    // apply() may have appended a pin comment to a `.env` this run did not
    // write; a kept file is reported as kept, not as one of init's writes.
    if env != EnvAction::Written {
        written.retain(|path| path != &env_path);
    }

    let value = serde_json::json!({
        "dir": dir,
        "written": written,
        "env": env,
        "chap_image_tag": chap_core.tag,
        "env_tag_moved": tag_moved.is_some(),
        "chap_compose_source": chap_core.source,
        "api_port": args.api_port,
        "api_url": project.api_url(),
        "api_port_busy": api_port_busy,
        "busy_ports": ports.busy.iter().map(|c| c.port).collect::<Vec<u16>>(),
        "claimed_ports": ports.claimed.iter().map(|c| c.port).collect::<Vec<u16>>(),
        "port_warnings": ports.lines,
        "components": project.state.components,
        // What the previous deployment in this directory lost, for a `--force`
        // that a script is driving rather than a person reading the warnings.
        "dropped_components": dropped.iter().map(|c| c.name()).collect::<Vec<&str>>(),
        "dropped_notes": dropped_notes,
        // Whether the deployment is protected, never the secrets themselves:
        // `--json` output is the kind of thing that ends up in a log.
        "auth": project.state.auth,
        "report": report,
        "adopted": adopted.as_ref().map(|lines| serde_json::json!({
            "from": adoption.as_ref().map(|a| a.dir.clone()),
            "compose_project": adoption.as_ref().map(|a| a.project.clone()),
            "lines": lines,
        })),
    });
    if !report_it {
        for warning in &warnings {
            crate::output::warn(warning);
        }
        return Ok(());
    }
    ctx.out.report(&value, |lines| {
        for warning in &warnings {
            lines.warning(warning.as_str());
        }
        if let (Some(adoption), Some(adopted)) = (&adoption, &adopted) {
            adoption_lines(adoption, adopted, lines);
        }
        summary(
            &dir,
            &written,
            &report,
            env,
            &chap_core,
            &project,
            secrets.as_ref(),
            &dropped_notes,
            tag_moved.as_ref(),
            lines,
        )
    })
}

#[cfg(test)]
mod tests;
