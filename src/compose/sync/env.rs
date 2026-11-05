//! The `.env` lines sync owns: the commented image pins and the sections each
//! enabled component appends, and nothing else in the operator's file.

use crate::components::{
    Components, DHIS2_DB_PASSWORD_ENV_VAR, DHIS2_DEFAULT_JAVA_OPTIONS,
    DHIS2_ENCRYPTION_PASSWORD_ENV_VAR, DHIS2_JAVA_ENV_VAR, DHIS2_SEED_ENV_VAR, DHIS2_TAG_ENV_VAR,
    OCS_DATA_SOURCE_ENV_VARS, OCS_TAG_ENV_VAR, S3_ACCESS_KEY_ENV_VAR, S3_SECRET_KEY_ENV_VAR,
    S3_TAG_ENV_VAR,
};
use crate::compose::render::NO_TAG_PINS;
use crate::compose::spec::Dhis2Spec;
use crate::compose::tag_env_var;
use crate::error::Result;
use crate::project::{CHAP_TAG_ENV_VAR, ENV_FILE, Project};
use std::path::{Path, PathBuf};

/// Add a commented image pin to `.env` for every enabled model that has none,
/// and the settings section of every enabled component that has none.
///
/// Only ever appends: the file belongs to the operator once `init` wrote it,
/// and it may hold passwords and tokens we must not rewrite. Returns the path
/// when something was (or with `check`, would be) appended.
pub(crate) fn append_env_pins(project: &Project, check: bool) -> Result<Option<PathBuf>> {
    let path = project.dir.join(ENV_FILE);
    let Ok(body) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    let mut pins = Vec::new();
    for (id, model) in &project.state.models {
        let var = tag_env_var(id);
        if !mentions_var(&body, &var) {
            pins.push(format!("# {var}={}", model.pinned_tag()));
        }
    }
    let components = component_env_sections(&project.state.components, &body)?;
    if pins.is_empty() && components.is_empty() {
        return Ok(None);
    }
    if check {
        return Ok(Some(path));
    }

    // The generated .env carries a placeholder under the pin heading so the
    // section is never a dangling title; the first real pin replaces it. With
    // no pin to put there it stays, or the heading would dangle instead.
    let mut out: String = body
        .lines()
        .filter(|line| pins.is_empty() || line.trim_end() != NO_TAG_PINS)
        .map(|line| format!("{line}\n"))
        .collect();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !pins.is_empty() {
        out.push_str(&pins.join("\n"));
        out.push('\n');
    }
    for section in &components {
        out.push('\n');
        out.push_str(section);
    }
    crate::dotenv::write(&path, &out)?;
    Ok(Some(path))
}

/// The `.env` sections the enabled components need and `body` does not have.
///
/// Each one is appended whole, with its own heading, so the file reads as
/// sections rather than as a tail of loose variables. The object store's
/// credentials are generated here and never rewritten: like the database
/// password, the volume they created is stamped with them.
fn component_env_sections(components: &Components, body: &str) -> Result<Vec<String>> {
    let mut sections = Vec::new();
    if components.ocs.enabled && !mentions_var(body, OCS_TAG_ENV_VAR) {
        sections.push(format!(
            "# OCS (component). Uncomment to pin a build; the default follows `main`.\n\
             # {OCS_TAG_ENV_VAR}={}\n",
            components.ocs.image_tag
        ));
    }
    // Placeholders only: a credential is the operator's to paste in, and an
    // ERA5-Land key that arrives here would have had to come from somewhere
    // this CLI has no business reading. The section exists so the variable
    // names are in the file an operator already edits rather than in the docs
    // alone, and so `chap-devops auth show` has something to report on.
    //
    // The heading wraps at 80 columns, unlike the notes this same fact is
    // printed in: a note scrolls past in a terminal, and `.env` is opened in an
    // editor. Neither line is what decides whether the section is already here -
    // that is `mentions_var` over the five variables below - so a deployment
    // whose `.env` carries the one-line heading this replaced is left alone
    // exactly as one carrying this heading is.
    if components.ocs.enabled
        && !OCS_DATA_SOURCE_ENV_VARS
            .iter()
            .any(|var| mentions_var(body, var))
    {
        let mut section = String::from(
            "# OCS data sources (optional): ERA5-Land needs one or both of ECMWF_DATASTORES_*\n\
             # and EDH_API_KEY, per dataset; WorldPop and CHIRPS3 need none.\n",
        );
        for var in OCS_DATA_SOURCE_ENV_VARS {
            let value = if *var == "ECMWF_DATASTORES_URL" {
                crate::components::OCS_ECMWF_URL
            } else {
                ""
            };
            section.push_str(&format!("# {var}={value}\n"));
        }
        sections.push(section);
    }
    if components.s3.enabled
        && !(mentions_var(body, S3_ACCESS_KEY_ENV_VAR) || mentions_var(body, S3_SECRET_KEY_ENV_VAR))
    {
        sections.push(format!(
            "# S3 (component). Root credentials, generated once; see the docs before changing them.\n\
             {S3_ACCESS_KEY_ENV_VAR}={}\n\
             {S3_SECRET_KEY_ENV_VAR}={}\n\
             # {S3_TAG_ENV_VAR}={}\n",
            crate::auth::random_hex(16)?,
            crate::auth::random_hex(16)?,
            crate::components::S3_DEFAULT_TAG,
        ));
    }
    // Two generated secrets, and the same rule the object store's follow: the
    // volume was created with them, so they are written once and never touched
    // again. `random_hex(16)` is 32 characters, comfortably past the 24 DHIS2
    // demands of the encryption password - under that it stops on
    // ENCRYPTION_PASSWORD_TOO_SHORT rather than starting with a weak key.
    //
    // The three commented lines below them are the pins an operator goes looking
    // for: the image, the dump the one-shot fetches and the JVM options the
    // compose file reads, each with the value the rendered file already defaults
    // to, so uncommenting one changes nothing until it is edited.
    if components.dhis2.enabled
        && !(mentions_var(body, DHIS2_DB_PASSWORD_ENV_VAR)
            || mentions_var(body, DHIS2_ENCRYPTION_PASSWORD_ENV_VAR))
    {
        // The value the rendered file already defaults to, which for a dump that
        // is a file is the path *inside* the container: the host path would be
        // one nothing in there can read.
        let spec = Dhis2Spec::from_components(components);
        let seed = spec
            .seed
            .as_ref()
            .map(crate::compose::render::seed_value)
            .unwrap_or_default();
        sections.push(format!(
            "# DHIS2 (component). Generated once; the database volume was created with them.\n\
             {DHIS2_DB_PASSWORD_ENV_VAR}={}\n\
             {DHIS2_ENCRYPTION_PASSWORD_ENV_VAR}={}\n\
             # {DHIS2_TAG_ENV_VAR}={}\n\
             # {DHIS2_SEED_ENV_VAR}={seed}\n\
             # {DHIS2_JAVA_ENV_VAR}={DHIS2_DEFAULT_JAVA_OPTIONS}\n",
            crate::auth::random_hex(16)?,
            crate::auth::random_hex(16)?,
            spec.image_tag,
        ));
    }
    // The credentials `chap-devops dhis2` authenticates with, as placeholders holding
    // the values it already falls back to - so uncommenting one changes nothing
    // until it is edited, the same rule the pins above follow.
    //
    // The variable names are the whole answer to "where do the credentials
    // come from", and a name nobody can find is a name nobody sets.
    // Nothing here is a secret: every line is commented, `admin` and `district`
    // are what a seeded dump and an empty database both give, and no container
    // is passed any of them. An external DHIS2 gets no `district`: chap-devops did not
    // create it, so there is no default to write down.
    let deployed = components.dhis2.enabled && components.dhis2_external.is_none();
    let wanted = components.dhis2.enabled || components.dhis2_external.is_some();
    let login_named = mentions_var(body, crate::dhis2::ADMIN_USERNAME_ENV_VAR)
        || mentions_var(body, crate::dhis2::ADMIN_PASSWORD_ENV_VAR);
    if wanted && !login_named {
        sections.push(format!(
            "# DHIS2 login `chap-devops dhis2` uses. Only chap-devops reads these - no container is given\n\
             # them - and {}. A personal access token,\n\
             # when set, is used instead of the username and password.\n\
             # {}=\n\
             # {}={}\n\
             # {}={}\n",
            match deployed {
                true => "commented out means the DHIS2 default below",
                false => "chap-devops has no default for a DHIS2 it did not deploy",
            },
            crate::dhis2::API_TOKEN_ENV_VAR,
            crate::dhis2::ADMIN_USERNAME_ENV_VAR,
            crate::dhis2::DEFAULT_USERNAME,
            crate::dhis2::ADMIN_PASSWORD_ENV_VAR,
            match deployed {
                true => crate::dhis2::DEFAULT_PASSWORD,
                false => "",
            },
        ));
    }
    Ok(sections)
}

/// Move the commented `# <VAR>=<tag>` pin line of `var` to `tag`.
///
/// Only that one comment line changes. An active (uncommented) `VAR=` line is
/// the operator's own override and is left alone, as is a `.env` that does
/// not mention the variable at all (the next sync appends it). Returns
/// whether the file changed.
pub fn refresh_env_pin(dir: &Path, var: &str, tag: &str) -> Result<bool> {
    let path = dir.join(ENV_FILE);
    let Ok(body) = std::fs::read_to_string(&path) else {
        return Ok(false);
    };
    let wanted = format!("# {var}={tag}");
    let mut changed = false;
    let mut out = String::with_capacity(body.len());
    for line in body.lines() {
        if is_commented_pin(line, var) && line != wanted {
            out.push_str(&wanted);
            changed = true;
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    if !changed {
        return Ok(false);
    }
    crate::dotenv::write(&path, &out)?;
    Ok(true)
}

/// What [`set_env_chap_tag`] found in `.env`, and therefore what the running
/// deployment will do with the new tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvTag {
    /// There is no `.env` (a project created with `init --no-env`).
    NoFile,
    /// The active `CHAP_IMAGE_TAG=` line now names the new tag.
    Updated,
    /// The only mention is a commented placeholder, so the deployment follows
    /// the compose default. Left alone: uncommenting it is the operator's call.
    Commented,
    /// An active line holds a value this project did not put there.
    Foreign(String),
    /// The file never mentions the variable.
    Absent,
}

/// Move the active `CHAP_IMAGE_TAG=` line of `.env` from `old` to `new`.
///
/// Exactly one line may change, and only when it still says what this project
/// recorded: an operator who pinned something else, or who left the generated
/// placeholder commented out, has made a decision that `chap-devops update` does not
/// get to undo. The outcome says which of those it was so the caller can warn.
pub fn set_env_chap_tag(dir: &Path, old: &str, new: &str) -> Result<EnvTag> {
    let path = dir.join(ENV_FILE);
    let Ok(body) = std::fs::read_to_string(&path) else {
        return Ok(EnvTag::NoFile);
    };
    // What the deployment is actually running, by compose's rules: the last
    // active assignment, `export ` and quotes included. Reading the first one
    // would let this rewrite a line the deployment is not using and report
    // that the tag had moved. See [`crate::dotenv`].
    let Some(value) = crate::dotenv::value(&body, CHAP_TAG_ENV_VAR) else {
        return Ok(if mentions_var(&body, CHAP_TAG_ENV_VAR) {
            EnvTag::Commented
        } else {
            EnvTag::Absent
        });
    };
    if value != old && value != new {
        return Ok(EnvTag::Foreign(value));
    }
    // The same rules writing: one active line survives, holding the new tag.
    let mut lines: Vec<String> = body.lines().map(str::to_string).collect();
    crate::dotenv::set(&mut lines, CHAP_TAG_ENV_VAR, new);
    let out = crate::dotenv::join(&lines);
    if out != body {
        crate::dotenv::write(&path, &out)?;
    }
    Ok(EnvTag::Updated)
}

/// Whether any line, commented or not, assigns `var`.
fn mentions_var(body: &str, var: &str) -> bool {
    body.lines().any(|line| {
        let line = line.trim_start().trim_start_matches('#').trim_start();
        line.starts_with(&format!("{var}="))
    })
}

/// `# VAR=...`, with any amount of whitespace around the `#`.
fn is_commented_pin(line: &str, var: &str) -> bool {
    let trimmed = line.trim_start();
    let Some(rest) = trimmed.strip_prefix('#') else {
        return false;
    };
    rest.trim_start().starts_with(&format!("{var}="))
}
