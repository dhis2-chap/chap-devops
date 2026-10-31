//! The `dhis2` component: its compose file, the seed-dump one-shot and the
//! scaffolded `dhis.conf`.

use super::labels::{ROLE_DHIS2, labels_block};
use super::{fill, normalize_newlines};
use crate::components::{DHIS2_CONFIG_FILE, DHIS2_DIR, DHIS2_TAG_ENV_VAR};
use crate::compose::spec::{Dhis2ConfigSpec, Dhis2SeedSource, Dhis2Spec};
use std::sync::LazyLock;

/// The `dhis2` component, as a compose file of its own.
pub(super) static DHIS2_TEMPLATE: LazyLock<String> =
    LazyLock::new(|| normalize_newlines(include_str!("../templates/compose.dhis2.yml")));
/// The `dhis.conf` a `dhis2` component is scaffolded with.
static DHIS2_CONFIG_TEMPLATE: LazyLock<String> =
    LazyLock::new(|| normalize_newlines(include_str!("../templates/dhis.conf")));

/// The one-shot that prepares the seed dump. A fragment rather than a file of
/// its own, like the model overlay's init container: a deployment that wants an
/// empty DHIS2 has no such service at all.
///
/// The two things it does that are not obvious are commented in place, because
/// both were learned from a restore that failed: published dumps are
/// `pg_dump --clean` without `--if-exists`, and a truncated download that gets
/// cached looks exactly like a dump with some of the data missing.
const DHIS2_DUMP_TEMPLATE: &str = r#"  dhis2-dump:
    # One-shot: fetches the seed dump, writes the scripts that load it into a
    # fresh database, and exits. dhis2-db mounts the same volume at
    # /docker-entrypoint-initdb.d/, so the restore is the stock postgres
    # entrypoint's - which runs it once, on a data directory it has just created,
    # and never again.
@LABELS@    image: @DUMP_IMAGE@
    restart: "no"
    working_dir: /opt/dump
    environment:
      DHIS2_DB_DUMP_URL: ${DHIS2_DB_DUMP_URL:-@SEED_URL@}
      @TAG_VAR@: ${@TAG_VAR@:-@IMAGE_TAG@}
@PASSWORD_ENV@    volumes:
      - type: volume
        source: @DUMP_VOLUME@
        target: /opt/dump
@SEED_MOUNT@    entrypoint: ["/bin/sh", "-c"]
    command:
      - |
        set -eu
@CLEAN_STEP@@PASSWORD_STEP@        # The last init script marks the database as restored. The health check
        # asks for the mark, so a restore that stopped half way stays unhealthy
        # after docker starts the database again and postgres skips the init.
        printf '%s\n' \
          "SELECT current_database() AS chap_devops_db \\gset" \
          "COMMENT ON DATABASE :\"chap_devops_db\" IS '@SEED_MARK@';" > @MARK_SCRIPT@
        # The rewrite and the restore are one pass: dump.sh, which the postgres
        # entrypoint runs in name order, streams the dump through sed into psql.
        # The dump is read once, and with the gzip and sed of the database
        # image, which are faster than those of busybox. seed.dump.gz does not
        # end in .sql.gz, so the entrypoint does not load it by itself.
        #
        # Published dumps are `pg_dump --clean` WITHOUT `--if-exists`, so they
        # open on DROP and ALTER statements that assume the schema is already
        # there, and the restore runs under `psql -v ON_ERROR_STOP=1`: the first
        # missing object aborts it. Retrofitting --if-exists makes that opening
        # block a no-op on an empty database. The substitutions are idempotent,
        # and each one is anchored to the start of a line, so COPY data rows are
        # left alone. DROP EXTENSION and DROP SCHEMA go entirely: the postgis
        # image owns those objects, and the dump recreates what it needs with
        # CREATE EXTENSION IF NOT EXISTS.
        # A dump from a newer pg_dump (17.6 and later, 16.10 and later) opens
        # with \restrict and closes with \unrestrict, which the psql of this
        # image does not know, and a dump from PostgreSQL 17 sets
        # transaction_timeout, which PostgreSQL 16 does not know. All three go.
        # The owners and the grants name the roles of the source server, which
        # do not exist here: they go too, as with pg_dump --no-owner
        # --no-privileges, and every object belongs to the database user.
        cat > @REWRITE_SCRIPT@ <<'EOF'
        /^\\restrict /d
        /^\\unrestrict /d
        /^SET transaction_timeout /d
        /^ALTER [^;]* OWNER TO /d
        /^(GRANT|REVOKE) /d
        /^ALTER DEFAULT PRIVILEGES /d
        s/^ALTER TABLE (IF EXISTS )?(ONLY )?/ALTER TABLE IF EXISTS \2/
        s/^DROP (TABLE|INDEX|SEQUENCE|FUNCTION|VIEW|MATERIALIZED VIEW|TYPE|DOMAIN|AGGREGATE|TRIGGER) (IF EXISTS )?/DROP \1 IF EXISTS /
        /^DROP EXTENSION /d
        /^DROP SCHEMA /d
        s/^CREATE SCHEMA (IF NOT EXISTS )?/CREATE SCHEMA IF NOT EXISTS /
        EOF
        # The entrypoint sources this file with `set -Eeo pipefail`, so a
        # truncated dump stops the restore, and the mark guard reports it.
        cat > @RESTORE_SCRIPT@ <<'EOF'
        echo "chap-devops: restoring @SEED_FILE@"
        gunzip -c /docker-entrypoint-initdb.d/@SEED_FILE@ \
          | sed -E -f /docker-entrypoint-initdb.d/@REWRITE_SCRIPT@ \
          | psql -v ON_ERROR_STOP=1 --username "$$POSTGRES_USER" --no-password --no-psqlrc --dbname "$$POSTGRES_DB"
        EOF
        # A dump an earlier chap-devops prepared would be loaded a second time.
        rm -f dump.sql.gz out.part
        if [ -f @SEED_FILE@ ]; then
          echo "@SEED_FILE@ is already there"
          exit 0
        fi
        rm -f raw.part
        # A value that names a file is the one mounted above, which is copied in
        # place of the download. The move at the end means that a download that
        # stopped is never kept as the seed.
        if [ -f "$${DHIS2_DB_DUMP_URL}" ]; then
          echo "copying $${DHIS2_DB_DUMP_URL}"
          cp "$${DHIS2_DB_DUMP_URL}" raw.part
        else
          echo "downloading $${DHIS2_DB_DUMP_URL}"
          wget -O raw.part "$${DHIS2_DB_DUMP_URL}"
        fi
@VERSION_CHECK@        mv raw.part @SEED_FILE@
        echo "wrote @SEED_FILE@ ($$(wc -c < @SEED_FILE@) bytes)""#;

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

/// The step of the one-shot that writes [`DHIS2_PASSWORD_SCRIPT`], or removes
/// it when no password is set. It runs before the check for a prepared dump,
/// so a change of the setting reaches the next restore.
///
/// PostgreSQL makes the bcrypt hash itself, with `pgcrypto`: one hash for all
/// the users, as a hash for each user takes minutes on a large dump.
///
/// When the dump has no `pgcrypto` extension, the script installs it in a
/// schema of its own and removes that schema after the update. The DHIS2 dumps
/// carry `gen_random_uuid` and other functions of `pgcrypto` in `public`
/// without the extension, so `CREATE EXTENSION pgcrypto` in `public` fails.
const PASSWORD_STEP: &str = r#"        # Every DHIS2 user gets the password in DHIS2_SEED_PASSWORD, and every
        # account is turned on, with no two-factor login and no expiry date. The
        # postgres entrypoint runs this file after the dump, because the files
        # run in name order.
        password=$$(printf '%s' "$${DHIS2_SEED_PASSWORD}" | sed "s/'/''/g")
        cat > @SCRIPT@ <<EOF
        SELECT NOT EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'pgcrypto') AS chap_devops_new_pgcrypto \gset
        \if :chap_devops_new_pgcrypto
        CREATE SCHEMA chap_devops_pgcrypto;
        CREATE EXTENSION pgcrypto SCHEMA chap_devops_pgcrypto;
        \endif
        SELECT extnamespace::regnamespace AS chap_devops_pgcrypto_schema FROM pg_extension WHERE extname = 'pgcrypto' \gset
        UPDATE userinfo SET password = h.hash, disabled = false
          FROM (SELECT :chap_devops_pgcrypto_schema.crypt('$$password', :chap_devops_pgcrypto_schema.gen_salt('bf', 10)) AS hash) AS h;
        SELECT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'userinfo' AND column_name = 'twofactortype') AS chap_devops_twofactor \gset
        \if :chap_devops_twofactor
        UPDATE userinfo SET twofactortype = 'NOT_ENABLED', secret = NULL;
        \endif
        SELECT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'userinfo' AND column_name = 'accountexpiry') AS chap_devops_expiry \gset
        \if :chap_devops_expiry
        UPDATE userinfo SET accountexpiry = NULL;
        \endif
        \if :chap_devops_new_pgcrypto
        DROP SCHEMA chap_devops_pgcrypto CASCADE;
        \endif
        EOF
        echo "wrote @SCRIPT@: the restore gives every user one password, with no two-factor login"
"#;

/// The check of the DHIS2 version of a downloaded dump, in the one-shot. Only
/// for a URL: `chap-devops init` reads a local dump itself, much faster than busybox
/// `gunzip`, which needs minutes to reach the Flyway table of a large dump.
const VERSION_CHECK: &str = r#"        # DHIS2 runs a database of its own minor version or older, and chap-devops
        # supports @OLDEST@ and newer. The newest Flyway migration says which
        # version the dump is. The check comes before the slow rewrite. A local
        # dump was checked by `chap-devops init` already, so only a URL gets it.
        dump_minor=$$(gunzip -c raw.part 2>/dev/null | grep -m1 -A5000 '^COPY public\.flyway_schema_history ' | awk -F'\t' 'NR == 1 {next} $$0 == "\\." {exit} $$2 ~ /^[0-9]/ {print $$2}' | sort -t. -k1,1n -k2,2n -k3,3n | tail -1 | cut -d. -f1,2)
        if [ -n "$$dump_minor" ]; then
          dump_n=$$(echo "$$dump_minor" | awk -F. '{print $$1 * 1000 + $$2}')
          tag_n=$$(echo "$${@TAG_VAR@}" | awk -F. '/^[0-9]+\.[0-9]+/ {print $$1 * 1000 + $$2}')
          echo "the dump is DHIS2 $$dump_minor (its newest Flyway migration)"
          if [ "$$dump_n" -lt @OLDEST_N@ ]; then
            echo "error: the dump is from DHIS2 $$dump_minor, and chap-devops supports DHIS2 @OLDEST@ and newer; upgrade the database in DHIS2 first"
            exit 1
          fi
          if [ -n "$$tag_n" ] && [ "$$tag_n" -lt "$$dump_n" ]; then
            echo "error: DHIS2 $${@TAG_VAR@} is older than the dump (DHIS2 $$dump_minor), and DHIS2 does not run a newer database; run chap-devops components enable dhis2 --tag $$dump_minor, then chap-devops up"
            exit 1
          fi
        fi
"#;

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

/// The step of the one-shot that writes [`DHIS2_CLEAN_SCRIPT`], for every
/// seed.
///
/// A real DHIS2 database has values that match the number pattern of
/// analytics but do not fit in `double precision`, such as a number of two
/// thousand digits. Analytics casts them and stops. The script lists them in
/// the log of `dhis2-db`, then empties them. Deleted rows too: the outlier
/// query of analytics reads `datavalue` without the `deleted` filter.
const CLEAN_STEP: &str = r#"        # Data values that analytics reads as numbers but that do not fit in a
        # double precision stop every analytics run. The restore lists them in
        # the log of dhis2-db, then empties them.
        cat > @SCRIPT@ <<'EOF'
        SELECT to_regclass('public.datavalue') IS NOT NULL AS chap_devops_has_datavalue \gset
        \if :chap_devops_has_datavalue
        CREATE TEMP TABLE chap_devops_unreadable AS
          SELECT dv.dataelementid, dv.periodid, dv.sourceid, dv.categoryoptioncomboid, dv.attributeoptioncomboid
          FROM datavalue dv
          WHERE dv.value ~ '^-?[0-9]+(\.[0-9]+)?$$'
            AND length(split_part(ltrim(dv.value, '-'), '.', 1)) > 300
            AND abs(dv.value::numeric) > 1.7976931348623157e308;
        \echo 'chap-devops: data values that analytics cannot read as a number, emptied:'
        SELECT de.uid AS dataelement, de.name, pe.startdate AS period, ou.uid AS orgunit,
               left(dv.value, 12) AS value_starts, length(dv.value) AS length
          FROM chap_devops_unreadable u
          JOIN datavalue dv USING (dataelementid, periodid, sourceid, categoryoptioncomboid, attributeoptioncomboid)
          JOIN dataelement de ON de.dataelementid = dv.dataelementid
          JOIN period pe ON pe.periodid = dv.periodid
          JOIN organisationunit ou ON ou.organisationunitid = dv.sourceid;
        UPDATE datavalue dv SET value = NULL
          FROM chap_devops_unreadable u
          WHERE dv.dataelementid = u.dataelementid AND dv.periodid = u.periodid
            AND dv.sourceid = u.sourceid AND dv.categoryoptioncomboid = u.categoryoptioncomboid
            AND dv.attributeoptioncomboid = u.attributeoptioncomboid;
        \endif
        EOF
"#;

fn clean_step() -> &'static str {
    static STEP: LazyLock<String> =
        LazyLock::new(|| CLEAN_STEP.replace("@SCRIPT@", DHIS2_CLEAN_SCRIPT));
    &STEP
}

/// The step for a deployment that keeps the passwords of the dump.
const NO_PASSWORD_STEP: &str = "        rm -f @SCRIPT@\n";

fn password_step(password: Option<&str>) -> &'static str {
    static WITH: LazyLock<String> =
        LazyLock::new(|| PASSWORD_STEP.replace("@SCRIPT@", DHIS2_PASSWORD_SCRIPT));
    static WITHOUT: LazyLock<String> =
        LazyLock::new(|| NO_PASSWORD_STEP.replace("@SCRIPT@", DHIS2_PASSWORD_SCRIPT));
    match password {
        Some(_) => &WITH,
        None => &WITHOUT,
    }
}

/// The `DHIS2_SEED_PASSWORD` line of the one-shot, with `.env` able to move
/// it. A `$` in the password is doubled, so compose does not read it as a
/// variable.
fn password_env_line(password: Option<&str>) -> String {
    match password {
        Some(password) => format!(
            "      DHIS2_SEED_PASSWORD: ${{DHIS2_SEED_PASSWORD:-{}}}\n",
            password.replace('$', "$$")
        ),
        None => String::new(),
    }
}

/// The init script that marks a restore as complete. It sorts after every
/// other file of the dump volume, so it runs last.
pub const DHIS2_SEED_MARK_SCRIPT: &str = "zzz-chap-devops-seeded.sql";

/// The comment the restore leaves on the database when it is complete.
pub const DHIS2_SEED_MARK: &str = "chap-devops: seed restored";

/// The health check of `dhis2-db`: the server answers over TCP, and on a
/// seeded deployment the database has the mark of a complete restore.
fn db_health_test(seeded: bool) -> String {
    let ready = "pg_isready -h 127.0.0.1 -U $${POSTGRES_USER} -d $${POSTGRES_DB}";
    match seeded {
        false => format!("      test: [\"CMD-SHELL\", \"{ready}\"]"),
        true => format!(
            "      # On a seeded deployment, also the mark of a complete restore: without\n\
             \x20     # it the restore stopped, and the database is incomplete.\n\
             \x20     test:\n\
             \x20       - CMD-SHELL\n\
             \x20       - >-\n\
             \x20         {ready} &&\n\
             \x20         psql -U $${{POSTGRES_USER}} -d $${{POSTGRES_DB}} -tAc\n\
             \x20         \"SELECT shobj_description(oid, 'pg_database') FROM pg_database\n\
             \x20         WHERE datname = current_database()\" | grep -qx '{DHIS2_SEED_MARK}'"
        ),
    }
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
    let labels = labels_block(ROLE_DHIS2, None, spec.group.as_deref());
    let mut port_lines = format!("    expose:\n      - \"{DHIS2_CONTAINER_PORT}\"\n");
    if let Some(port) = spec.host_port {
        port_lines.push_str(&format!(
            "    ports:\n      - \"{port}:{DHIS2_CONTAINER_PORT}\"\n"
        ));
    }
    if spec.host_gateway {
        port_lines.push_str(&format!(
            "    extra_hosts:\n      - \"{}:host-gateway\"\n",
            crate::components::HOST_GATEWAY
        ));
    }
    // Every seed block starts at the beginning of its line and carries its own
    // newlines, so a deployment without one leaves neither a comment nor a blank
    // line behind.
    let (dump_mount, dump_depends, dump_service, dump_volume_line) = match &spec.seed {
        None => (
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ),
        Some(seed) => (
            format!(
                "      # Where the postgres entrypoint looks for init scripts: the restore is\n\
                 \x20     # its own, on a data directory it has just created, and happens once.\n\
                 \x20     # Docker copies the image's postgis script into this volume as the\n\
                 \x20     # container is created, so that still runs, ahead of the dump.\n\
                 \x20     - type: volume\n\
                 \x20       source: {DHIS2_DUMP_VOLUME}\n\
                 \x20       target: /docker-entrypoint-initdb.d/\n"
            ),
            "    depends_on:\n      dhis2-dump:\n        condition: service_completed_successfully\n"
                .to_string(),
            format!(
                "\n{}\n",
                fill(
                    DHIS2_DUMP_TEMPLATE,
                    &[
                        ("LABELS", &labels),
                        ("DUMP_IMAGE", DHIS2_DUMP_IMAGE),
                        ("SEED_URL", seed_value(seed)),
                        ("SEED_MOUNT", &seed_mount_line(seed)),
                        ("VERSION_CHECK", match seed {
                            Dhis2SeedSource::Url(_) => VERSION_CHECK,
                            Dhis2SeedSource::File(_) => "",
                        }),
                        ("SEED_FILE", DHIS2_SEED_FILE),
                        ("RESTORE_SCRIPT", DHIS2_RESTORE_SCRIPT),
                        ("REWRITE_SCRIPT", DHIS2_REWRITE_SCRIPT),
                        ("SEED_MARK", DHIS2_SEED_MARK),
                        ("TAG_VAR", DHIS2_TAG_ENV_VAR),
                        ("IMAGE_TAG", &spec.image_tag),
                        ("OLDEST", crate::components::DHIS2_OLDEST_MINOR),
                        ("OLDEST_N", &oldest_number()),
                        ("MARK_SCRIPT", DHIS2_SEED_MARK_SCRIPT),
                        ("PASSWORD_ENV", &password_env_line(spec.seed_password.as_deref())),
                        ("CLEAN_STEP", clean_step()),
                        ("PASSWORD_STEP", password_step(spec.seed_password.as_deref())),
                        ("DUMP_VOLUME", DHIS2_DUMP_VOLUME),
                    ],
                )
            ),
            format!("  {DHIS2_DUMP_VOLUME}: {{}}\n"),
        ),
    };
    fill(
        &DHIS2_TEMPLATE,
        &[
            ("LABELS", &labels),
            ("IMAGE", &spec.image),
            ("TAG_VAR", DHIS2_TAG_ENV_VAR),
            ("IMAGE_TAG", &spec.image_tag),
            ("PORT_LINES", &port_lines),
            ("CONTAINER_PORT", &DHIS2_CONTAINER_PORT.to_string()),
            ("CONFIG_PATH", &format!("{DHIS2_DIR}/{DHIS2_CONFIG_FILE}")),
            ("DB_IMAGE", DHIS2_DB_IMAGE),
            ("HOME_VOLUME", DHIS2_HOME_VOLUME),
            ("DB_VOLUME", DHIS2_DB_VOLUME),
            ("DUMP_MOUNT", &dump_mount),
            ("DB_HEALTH_TEST", &db_health_test(spec.seed.is_some())),
            ("DUMP_DEPENDS", &dump_depends),
            ("DUMP_SERVICE", &dump_service),
            ("DUMP_VOLUME_LINE", &dump_volume_line),
        ],
    )
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

/// The bind mount a file seed needs, and nothing at all for a URL.
///
/// The token sits at the start of its line and carries its own newline, so a URL
/// seed leaves no blank line in the volume list.
///
/// A relative path is spelled with a leading `./`: compose resolves it against
/// the project directory either way, but only that spelling is the one its own
/// documentation guarantees, and it is the spelling an operator recognises as a
/// path rather than a named volume.
fn seed_mount_line(seed: &Dhis2SeedSource) -> String {
    match seed {
        Dhis2SeedSource::Url(_) => String::new(),
        Dhis2SeedSource::File(path) => {
            let path = path.replace('\\', "/");
            // Judged on the string, not by the host: the compose file reads the
            // same wherever it was rendered, and `Path::is_absolute` on Windows
            // calls `/srv/dump.sql.gz` relative and would make it `.//srv/...`.
            let drive = path.as_bytes();
            let absolute = path.starts_with('/')
                || (drive.len() > 2 && drive[0].is_ascii_alphabetic() && &drive[1..3] == b":/");
            let source = if absolute || path.starts_with('.') {
                path
            } else {
                format!("./{path}")
            };
            format!("      - {source}:{DHIS2_SEED_MOUNT}:ro\n")
        }
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
