//! The `dhis2` component: its compose file, the seed-dump one-shot and the
//! scaffolded `dhis.conf`.

use super::labels::{ROLE_DHIS2, labels_block};
use super::{fill, normalize_newlines, render_template};
use crate::components::{DHIS2_CONFIG_FILE, DHIS2_DIR, DHIS2_TAG_ENV_VAR};
use crate::compose::spec::{Dhis2ConfigSpec, Dhis2SeedSource, Dhis2Spec};
use std::sync::LazyLock;

/// The `dhis2` component, as a compose file of its own.
pub(super) static DHIS2_TEMPLATE: LazyLock<String> =
    LazyLock::new(|| normalize_newlines(include_str!("../templates/compose.dhis2.yml")));
/// The `dhis.conf` a `dhis2` component is scaffolded with.
static DHIS2_CONFIG_TEMPLATE: LazyLock<String> =
    LazyLock::new(|| normalize_newlines(include_str!("../templates/dhis.conf")));

/// The dump as the one-shot keeps it in the dump volume. Not `*.sql.gz`, so
/// the postgres entrypoint does not load it by itself.
pub const DHIS2_SEED_FILE: &str = "seed.dump.gz";

/// The init script that streams [`DHIS2_SEED_FILE`] through the rewrite into
/// psql. It sorts before the `zz-` scripts, which need the restored database.
pub const DHIS2_RESTORE_SCRIPT: &str = "dump.sh";

/// The sed program of the rewrite, in the dump volume.
pub const DHIS2_REWRITE_SCRIPT: &str = "chap-devops-rewrite.sed";

/// The init script that gives every DHIS2 user one password. The postgres
/// entrypoint runs the files in `/docker-entrypoint-initdb.d/` in name order,
/// so this name sorts after `dump.sql.gz`.
pub const DHIS2_PASSWORD_SCRIPT: &str = "zz-chap-devops-passwords.sql";

/// [`crate::components::DHIS2_OLDEST_MINOR`] as the number the shell
/// compares: `2.41` is 2041.
fn oldest_number() -> String {
    match crate::dhis2::version_parts(crate::components::DHIS2_OLDEST_MINOR).as_slice() {
        [major, minor, ..] => (major * 1000 + minor).to_string(),
        _ => "0".to_string(),
    }
}

/// The init script that empties the data values analytics cannot read.
pub const DHIS2_CLEAN_SCRIPT: &str = "zz-chap-devops-clean.sql";

/// The init script that marks a restore as complete. It sorts after every
/// other file of the dump volume, so it runs last.
pub const DHIS2_SEED_MARK_SCRIPT: &str = "zzz-chap-devops-seeded.sql";

/// The comment the restore leaves on the database when it is complete.
pub const DHIS2_SEED_MARK: &str = "chap-devops: seed restored";

/// The time `dhis2-db` gets before a failed health check counts, in seconds.
///
/// On a seeded deployment the first start is the restore. Measured: the Laos
/// demo dump (7.5 MB) in 7 seconds under amd64 emulation, about 1 second a MB,
/// and a national dump (3.9 GB) in 24 to 40 minutes, about 0.5 seconds a MB.
/// The restore gets twice the slower rate, and never less than 600 seconds.
pub(crate) fn db_start_period(seed_bytes: Option<u64>) -> u64 {
    const FLOOR: u64 = 600;
    seed_bytes.map_or(FLOOR, |bytes| FLOOR.max(bytes.div_ceil(1 << 20) * 2))
}

/// Container port DHIS2 listens on. Fixed, not a preference: the image's
/// `server.xml` declares one connector, on 8080, with the context path at the
/// root.
pub const DHIS2_CONTAINER_PORT: u16 = 8080;
/// Database image the component runs. PostGIS is not an optimisation here:
/// DHIS2's own startup check throws without the `postgis`, `pg_trgm` and
/// `btree_gin` extensions. The image is the PostGIS project's own, which is
/// amd64-only, so the service pins `linux/amd64` like a model overlay does.
pub const DHIS2_DB_IMAGE: &str = "postgis/postgis:16-3.5";
/// Image the seed-dump one-shot runs: it needs `wget`, `gzip` and `sed -E` and
/// nothing else.
pub const DHIS2_DUMP_IMAGE: &str = "alpine:3.24";
/// Where a seed dump that is a file rather than a URL is mounted inside the
/// one-shot.
///
/// One fixed path, whatever the file is called on the host, so the script has a
/// single name to test for: `[ -f ]` on the value of `DHIS2_DB_DUMP_URL` is what
/// tells a mounted file from a URL to download, and no second variable decides
/// it.
pub const DHIS2_SEED_MOUNT: &str = "/opt/seed.sql.gz";
/// Named volume holding `/opt/dhis2`: the installed apps and the file store,
/// measured at about 429 MB on a real instance. Not optional - without it both
/// are lost on every recreate.
pub const DHIS2_HOME_VOLUME: &str = "dhis2_home";
/// Named volume holding the DHIS2 database's data directory.
pub const DHIS2_DB_VOLUME: &str = "dhis2_db";
/// Named volume the seed dump is prepared in, mounted into the database at
/// `/docker-entrypoint-initdb.d/`.
///
/// A download cache rather than data: the one-shot fetches the dump again when
/// this is empty. [`crate::components::Component::volumes`] still names it, so
/// the doctor does not call it a leftover and `--purge` takes it;
/// [`crate::backup::COMPONENT_VOLUMES`] leaves it out, so no archive carries it.
pub const DHIS2_DUMP_VOLUME: &str = "dhis2_dump";
/// The dump the `2.42` line is seeded from unless `.env` names another: the
/// DHIS2 climate demo, which is the database the Modeling App's own walkthrough
/// assumes.
///
/// The entry for that line in [`crate::components::DHIS2_SEED_DUMPS`], which is
/// where the other lines' dumps are and where a tag is looked up.
pub const DHIS2_DEFAULT_SEED_URL: &str =
    "https://databases.dhis2.org/climate/laos/2.42/laos.sql.gz";

/// Render `compose.dhis2.yml`: the `dhis2` component.
///
/// Four services and three volumes when the database is seeded from a dump, and
/// three of each without one. `dhis2` is the web service and everything else is
/// `dhis2-<something>`, so one glance at a container name says which component
/// owns it.
///
/// The seed is the only conditional part, and it is four blocks that travel
/// together: the one-shot that prepares the dump, the volume it writes into, the
/// database's mount of that volume at `/docker-entrypoint-initdb.d/`, and the
/// database's wait on the one-shot. Without a seed none of them is rendered, and
/// DHIS2 migrates an empty database on first boot instead.
///
/// A seed that is a file rather than a URL adds one bind mount to the one-shot
/// and changes nothing else: the mounted path is what `DHIS2_DB_DUMP_URL`
/// carries, and the script's `[ -f ]` on that value is what picks the copy over
/// the download. So both shapes go through the same verification and the same
/// rewriting, and `.env` can still move either of them.
///
/// The host port is published only when the component records one, as with
/// `ocs`: an instance behind a reverse proxy is `expose`d on the compose network
/// and nowhere else. `dhis2-db` and `dhis2-prep` run the PostGIS image, which is
/// not multi-arch, so they have a `platform:` line; `dhis2/core` and `alpine`
/// are multi-arch.
pub fn render_dhis2(spec: &Dhis2Spec) -> String {
    let seed = spec.seed.as_ref();
    render_template(
        "compose.dhis2.yml",
        &DHIS2_TEMPLATE,
        minijinja::context! {
            labels => labels_block(ROLE_DHIS2, None, spec.group.as_deref()),
            image => &spec.image,
            tag_var => DHIS2_TAG_ENV_VAR,
            image_tag => &spec.image_tag,
            container_port => DHIS2_CONTAINER_PORT,
            host_port => spec.host_port,
            host_gateway => spec.host_gateway.then_some(crate::components::HOST_GATEWAY),
            config_path => format!("{DHIS2_DIR}/{DHIS2_CONFIG_FILE}"),
            db_image => DHIS2_DB_IMAGE,
            home_volume => DHIS2_HOME_VOLUME,
            db_volume => DHIS2_DB_VOLUME,
            start_var => crate::components::DHIS2_DB_START_ENV_VAR,
            db_start_period => format!("{}s", db_start_period(spec.seed_bytes)),
            seed => seed.is_some(),
            seed_is_url => matches!(seed, Some(Dhis2SeedSource::Url(_))),
            seed_url => seed.map(seed_value),
            seed_source => seed.and_then(seed_source),
            seed_mount => DHIS2_SEED_MOUNT,
            // A `$` is doubled, so compose does not read it as a variable.
            seed_password => spec.seed_password.as_ref().map(|p| p.replace('$', "$$")),
            dump_image => DHIS2_DUMP_IMAGE,
            dump_volume => DHIS2_DUMP_VOLUME,
            seed_file => DHIS2_SEED_FILE,
            seed_mark => DHIS2_SEED_MARK,
            mark_script => DHIS2_SEED_MARK_SCRIPT,
            rewrite_script => DHIS2_REWRITE_SCRIPT,
            restore_script => DHIS2_RESTORE_SCRIPT,
            password_script => DHIS2_PASSWORD_SCRIPT,
            clean_script => DHIS2_CLEAN_SCRIPT,
            oldest => crate::components::DHIS2_OLDEST_MINOR,
            oldest_n => oldest_number(),
        },
    )
}

/// The host side of the bind mount a file seed needs.
///
/// A relative path is spelled with a leading `./`: compose resolves it against
/// the project directory either way, but only that spelling is the one its own
/// documentation guarantees, and it is the spelling an operator recognises as a
/// path rather than a named volume.
fn seed_source(seed: &Dhis2SeedSource) -> Option<String> {
    let Dhis2SeedSource::File(path) = seed else {
        return None;
    };
    let path = path.replace('\\', "/");
    // Judged on the string, not by the host: the compose file reads the
    // same wherever it was rendered, and `Path::is_absolute` on Windows
    // calls `/srv/dump.sql.gz` relative and would make it `.//srv/...`.
    let drive = path.as_bytes();
    let absolute = path.starts_with('/')
        || (drive.len() > 2 && drive[0].is_ascii_alphabetic() && &drive[1..3] == b":/");
    Some(if absolute || path.starts_with('.') {
        path
    } else {
        format!("./{path}")
    })
}

/// What the one-shot's `DHIS2_DB_DUMP_URL` default carries: the URL to download,
/// or the path the mounted file has inside the container.
///
/// Public because `.env` writes the same value as the commented pin an operator
/// uncomments to move the seed. A mounted file's *host* path there would be a
/// path nothing inside the container can read, so both come from here.
pub fn seed_value(seed: &Dhis2SeedSource) -> &str {
    match seed {
        Dhis2SeedSource::Url(url) => url,
        Dhis2SeedSource::File(_) => DHIS2_SEED_MOUNT,
    }
}

/// Render the scaffolded `dhis2/dhis.conf`.
///
/// Written once, when the component is enabled, and never again: it is the
/// operator's file from that moment on, exactly like `ocs/climate-service.yaml`.
///
/// The file is mandatory - DHIS2 throws on startup without it, and there is no
/// environment-only mode - and what the environment does is substitute `${...}`
/// placeholders into it, so the placeholders are written literally and the
/// compose file passes the matching variables. `fill` only ever replaces `@KEY@`,
/// which is why the two notations can share one file.
///
/// The only value filled here is the route allowlist, because it is the only one
/// a caller has to decide: DHIS2 v42 and later default it to `https://*` and then
/// refuse the `http://` target the Modeling App needs, since it reaches chap-core
/// through a DHIS2 Route rather than directly.
pub fn render_dhis2_config(spec: &Dhis2ConfigSpec) -> String {
    fill(
        &DHIS2_CONFIG_TEMPLATE,
        &[("ROUTE_ALLOWED", &spec.route_allowed)],
    )
}
