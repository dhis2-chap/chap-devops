use super::check::{check_path, disabled_by};
use super::install::{backup_path, get_text, map_error, sha256_for, staged_path};
use super::release::{parse_release, release_at, same_commit};
use super::*;
use crate::chapcore::sha256_hex;
use crate::error::ChapError;
use std::path::{Path, PathBuf};

const LINUX: &str = "x86_64-unknown-linux-musl";
const MAC: &str = "aarch64-apple-darwin";
const WINDOWS: &str = "x86_64-pc-windows-msvc";

#[test]
fn every_target_names_its_archive() {
    assert_eq!(
        asset_name(LINUX),
        "chap-devops-x86_64-unknown-linux-musl.tar.gz"
    );
    assert_eq!(
        asset_name("aarch64-unknown-linux-musl"),
        "chap-devops-aarch64-unknown-linux-musl.tar.gz"
    );
    assert_eq!(
        asset_name(WINDOWS),
        "chap-devops-x86_64-pc-windows-msvc.zip"
    );
    assert_eq!(
        asset_name("aarch64-pc-windows-msvc"),
        "chap-devops-aarch64-pc-windows-msvc.zip"
    );
    // Nothing in the name says which release it came from.
    for target in [LINUX, MAC, WINDOWS] {
        assert!(!asset_name(target).contains("v0."), "{target}");
    }
}

#[test]
fn both_macs_take_the_universal_archive() {
    for target in [MAC, "x86_64-apple-darwin", "universal-apple-darwin"] {
        assert_eq!(asset_target(target), "universal-apple-darwin", "{target}");
        assert_eq!(
            asset_name(target),
            "chap-devops-universal-apple-darwin.tar.gz",
            "{target}"
        );
    }
    // Nothing else is rewritten.
    assert_eq!(asset_target(LINUX), LINUX);
    assert_eq!(asset_target(WINDOWS), WINDOWS);
}

#[test]
fn the_executable_inside_gets_an_exe_on_windows() {
    assert_eq!(binary_name(LINUX), "chap-devops");
    assert_eq!(binary_name(MAC), "chap-devops");
    assert_eq!(binary_name(WINDOWS), "chap-devops.exe");
    assert_eq!(binary_name("aarch64-pc-windows-msvc"), "chap-devops.exe");
}

#[test]
fn this_build_knows_its_own_target() {
    assert!(!TARGET.is_empty());
    assert_ne!(TARGET, "unknown");
    // Whatever the host is, the asset it wants exists in the release
    // matrix, which is the property that matters.
    assert!(asset_name(TARGET).starts_with("chap-devops-"));
    assert!(asset_name(TARGET).contains(asset_target(TARGET)));
}

#[test]
fn a_release_payload_yields_its_tag_and_assets() {
    let release = parse_release(
        r#"{
              "tag_name": "v0.2.0",
              "assets": [
                {"name": "chap-devops-x86_64-unknown-linux-musl.tar.gz"},
                {"name": "SHA256SUMS"}
              ]
            }"#,
    )
    .unwrap();
    assert_eq!(release.tag, "v0.2.0");
    assert!(release.has_asset("SHA256SUMS"));
    assert!(release.has_asset("chap-devops-x86_64-unknown-linux-musl.tar.gz"));
    assert!(!release.has_asset("chap-devops-x86_64-pc-windows-msvc.zip"));
}

#[test]
fn a_release_without_a_tag_is_an_error() {
    // The rate-limit body parses as JSON but carries no tag.
    let err = parse_release(r#"{"message":"API rate limit exceeded"}"#).unwrap_err();
    assert!(err.to_string().contains("tag_name"), "{err}");
    assert!(parse_release("<html>nope</html>").is_err());
    // Assets are optional; a release with none still parses.
    assert_eq!(
        parse_release(r#"{"tag_name":"v1.0.0"}"#).unwrap().assets,
        Vec::<String>::new()
    );
}

#[test]
fn the_asset_is_the_version_less_name() {
    let full = Release {
        tag: "v0.2.0".to_string(),
        assets: vec![
            "chap-devops-x86_64-unknown-linux-musl.tar.gz".to_string(),
            SUMS_FILE.to_string(),
        ],
        ..Default::default()
    };
    assert_eq!(
        pick_asset(&full, LINUX).unwrap(),
        "chap-devops-x86_64-unknown-linux-musl.tar.gz"
    );

    // A payload with no asset list is taken at its word.
    let bare = Release {
        assets: Vec::new(),
        ..full.clone()
    };
    assert_eq!(
        pick_asset(&bare, LINUX).unwrap(),
        "chap-devops-x86_64-unknown-linux-musl.tar.gz"
    );

    // A release that really has nothing for this target names the archive
    // it looked for, not the long list of what it has.
    let err = pick_asset(&full, WINDOWS).expect_err("no windows archive");
    assert_eq!(
        err.to_string(),
        format!(
            "v0.2.0 has no chap-devops-{WINDOWS}.zip, so chap-devops cannot install it on {WINDOWS}"
        )
    );
}

#[test]
fn newer_is_compared_against_this_build() {
    assert!(!is_newer_than_current(&format!("v{VERSION}")));
    assert!(!is_newer_than_current("v0.0.1"));
    assert!(is_newer_than_current("v999.0.0"));
    // A moving tag is not a point on the version line.
    assert!(!is_newer_than_current("latest"));
}

#[test]
fn the_install_method_is_guessed_from_the_path() {
    let method = |path: &Path| install_method_of(Some(path), false);
    let cargo: PathBuf = [r"/home/u", ".cargo", "bin", "chap-devops"]
        .iter()
        .collect();
    assert_eq!(method(&cargo), "cargo install");
    let cargo_win: PathBuf = [r"C:\Users\u", ".cargo", "bin", "chap-devops.exe"]
        .iter()
        .collect();
    assert_eq!(method(&cargo_win), "cargo install");
    // A local build anywhere else is not a release archive: `cargo install
    // --root` and `make install` put it in a plain `bin/`.
    for other in [
        "/usr/local/bin/chap-devops",
        "/home/u/.local/bin/chap-devops",
    ] {
        assert_eq!(method(Path::new(other)), "local build", "{other}");
    }
    // A checkout's own build, in either profile and on Windows too.
    for built in [
        &["/home/u/chap-devops", "target", "release", "chap-devops"][..],
        &[r"C:\src\chap-devops", "target", "debug", "chap-devops.exe"][..],
    ] {
        let built: PathBuf = built.iter().collect();
        assert_eq!(method(&built), "cargo build", "{built:?}");
    }
    // `.cargo` without `bin` under it is not a cargo install.
    assert_eq!(
        method(Path::new("/home/u/.cargo/chap-devops")),
        "local build"
    );
    assert_eq!(install_method_of(None, false), "local build");
}

/// A binary the release workflow built is a release archive wherever it is,
/// and this test binary is not one.
#[test]
fn a_release_build_is_a_release_archive_wherever_it_is() {
    for path in [
        "/usr/local/bin/chap-devops",
        "/home/u/.cargo/bin/chap-devops",
    ] {
        assert_eq!(
            install_method_of(Some(Path::new(path)), true),
            "release archive"
        );
    }
    assert_eq!(install_method_of(None, true), "release archive");
    // This test binary is a local build, so it is not reported as one.
    assert_eq!(
        install_method(Some(Path::new("/usr/local/bin/chap-devops"))),
        "local build"
    );
}

/// A `SHA256SUMS` in both spellings sha256sum and shasum write.
const SUMS: &str = "\
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  chap-devops-x86_64-unknown-linux-musl.tar.gz
ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad *chap-devops-universal-apple-darwin.tar.gz
248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1  ./chap-devops-x86_64-pc-windows-msvc.zip
";

#[test]
fn the_sums_file_is_read_in_every_spelling() {
    assert_eq!(
        sha256_for(SUMS, "chap-devops-x86_64-unknown-linux-musl.tar.gz").unwrap(),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_for(SUMS, "chap-devops-universal-apple-darwin.tar.gz").unwrap(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        sha256_for(SUMS, "chap-devops-x86_64-pc-windows-msvc.zip").unwrap(),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
    assert_eq!(
        sha256_for(SUMS, "chap-devops-aarch64-apple-darwin.tar.gz"),
        None
    );
    assert_eq!(sha256_for("", "anything"), None);
    // A line that is not a digest is not a match.
    assert_eq!(
        sha256_for("nothex  chap-devops.tar.gz", "chap-devops.tar.gz"),
        None
    );
}

#[test]
fn verification_passes_for_the_listed_bytes_and_fails_otherwise() {
    verify(SUMS, "chap-devops-x86_64-unknown-linux-musl.tar.gz", b"")
        .expect("the empty digest is the one listed");
    verify(SUMS, "chap-devops-universal-apple-darwin.tar.gz", b"abc")
        .expect("the abc digest is the one listed");

    let err = verify(SUMS, "chap-devops-universal-apple-darwin.tar.gz", b"abd")
        .expect_err("a changed byte is a different digest");
    assert!(err.to_string().contains("does not match"), "{err}");

    let err = verify(SUMS, "chap-devops-nowhere.tar.gz", b"")
        .expect_err("an asset the manifest never listed");
    assert!(err.to_string().contains("does not list"), "{err}");
}

#[test]
fn the_staged_and_backup_files_sit_beside_the_binary() {
    let unix = Path::new("/usr/local/bin/chap-devops");
    assert_eq!(
        staged_path(unix),
        Path::new("/usr/local/bin/chap-devops.new")
    );
    assert_eq!(
        backup_path(unix),
        Path::new("/usr/local/bin/chap-devops.old")
    );

    // The `.exe` is dropped, so Windows stages `chap-devops.new`, not
    // `chap-devops.exe.new`.
    let windows: PathBuf = ["bin", "chap-devops.exe"].iter().collect();
    assert_eq!(
        staged_path(&windows).file_name().unwrap(),
        std::ffi::OsStr::new("chap-devops.new")
    );
    assert_eq!(
        backup_path(&windows).file_name().unwrap(),
        std::ffi::OsStr::new("chap-devops.old")
    );
}

#[test]
fn replacing_a_binary_swaps_the_contents_and_leaves_nothing_behind() {
    let tmp = tempfile::tempdir().unwrap();
    let binary = tmp.path().join("chap-devops");
    std::fs::write(&binary, b"the old binary").unwrap();
    #[cfg(unix)]
    set_mode(&binary, 0o755);

    replace_executable(&binary, b"the new binary").expect("the swap succeeds");

    assert_eq!(std::fs::read(&binary).unwrap(), b"the new binary");
    assert!(!staged_path(&binary).exists(), "the staged file is gone");

    // Unix renames straight over the old binary, so nothing is parked
    // beside it and the mode it had is carried across.
    #[cfg(unix)]
    {
        assert!(!backup_path(&binary).exists(), "nothing is parked");
        assert_eq!(mode_of(&binary), 0o755);
    }

    // Windows cannot rename over a running image, so the old binary is
    // moved aside and the next run is the first that can delete it.
    #[cfg(windows)]
    {
        assert_eq!(
            std::fs::read(backup_path(&binary)).unwrap(),
            b"the old binary"
        );
        clean_backup(&binary);
        assert!(!backup_path(&binary).exists(), "the next run clears it");
    }
}

/// Modes are a Unix thing: Windows has no 0o750 to preserve.
#[cfg(unix)]
#[test]
fn replacing_preserves_a_mode_that_is_not_the_default() {
    let tmp = tempfile::tempdir().unwrap();
    let binary = tmp.path().join("chap-devops");
    std::fs::write(&binary, b"old").unwrap();
    set_mode(&binary, 0o750);

    replace_executable(&binary, b"new").unwrap();

    assert_eq!(std::fs::read(&binary).unwrap(), b"new");
    assert_eq!(mode_of(&binary), 0o750);
}

#[test]
fn a_leftover_backup_is_cleaned_up_and_a_missing_one_is_not_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let binary = tmp.path().join("chap-devops");
    std::fs::write(&binary, b"current").unwrap();

    clean_backup(&binary);
    std::fs::write(backup_path(&binary), b"previous").unwrap();
    clean_backup(&binary);
    assert!(!backup_path(&binary).exists());
    assert!(binary.exists(), "the current binary is untouched");
}

#[test]
fn a_writable_directory_is_replaceable_and_a_missing_one_is_not() {
    let tmp = tempfile::tempdir().unwrap();
    let binary = tmp.path().join("chap-devops");
    std::fs::write(&binary, b"x").unwrap();
    assert!(is_replaceable(&binary));
    assert!(!is_replaceable(
        &tmp.path().join("nowhere").join("chap-devops")
    ));

    // The probe it writes is cleaned up, whatever the platform thinks of
    // deleting a file that is still open.
    let left: Vec<String> = std::fs::read_dir(tmp.path())
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".chap-devops-write-probe"))
        .collect();
    assert!(left.is_empty(), "the probe is gone, found {left:?}");
}

#[test]
fn the_binary_is_found_wherever_the_archive_put_it() {
    let tmp = tempfile::tempdir().unwrap();
    let inner = tmp
        .path()
        .join("chap-devops-v0.2.0-x86_64-unknown-linux-musl");
    std::fs::create_dir_all(inner.join("completions")).unwrap();
    std::fs::write(inner.join("README.md"), "readme").unwrap();
    std::fs::write(inner.join("completions/chap-devops.bash"), "completions").unwrap();
    std::fs::write(inner.join("chap-devops"), b"elf").unwrap();

    assert_eq!(
        find_binary(tmp.path(), "x86_64-unknown-linux-musl"),
        Some(inner.join("chap-devops"))
    );
    // A Windows archive has `chap-devops.exe` and nothing called `chap-devops`.
    assert_eq!(find_binary(tmp.path(), "x86_64-pc-windows-msvc"), None);
}

#[test]
fn staleness_is_measured_against_the_interval() {
    let day = CHECK_INTERVAL.as_secs();
    let now = 1_000_000;
    assert!(is_stale(None, now, CHECK_INTERVAL), "no state at all");

    let fresh = CheckState {
        checked_at_unix: now - 60,
        latest: "v0.2.0".to_string(),
        ..CheckState::default()
    };
    assert!(!is_stale(Some(&fresh), now, CHECK_INTERVAL));

    let stale = CheckState {
        checked_at_unix: now - day - 1,
        latest: "v0.2.0".to_string(),
        ..CheckState::default()
    };
    assert!(is_stale(Some(&stale), now, CHECK_INTERVAL));

    // Exactly the interval counts as due.
    let due = CheckState {
        checked_at_unix: now - day,
        ..CheckState::default()
    };
    assert!(is_stale(Some(&due), now, CHECK_INTERVAL));

    // A timestamp from the future is a clock that moved, not a reason to
    // stop checking forever.
    let future = CheckState {
        checked_at_unix: now + day,
        ..CheckState::default()
    };
    assert!(is_stale(Some(&future), now, CHECK_INTERVAL));
}

#[test]
fn the_check_state_round_trips_through_the_cache_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("nested").join("cache");
    assert!(read_check(&cache).is_none(), "nothing written yet");

    let state = CheckState {
        checked_at_unix: 1_700_000_000,
        latest: "v0.3.0".to_string(),
        ..CheckState::default()
    };
    write_check(&cache, &state);
    let read = read_check(&cache).expect("what was just written");
    assert_eq!(read.checked_at_unix, 1_700_000_000);
    assert_eq!(read.latest, "v0.3.0");
    assert!(check_path(&cache).ends_with(CHECK_FILE));

    // A corrupt file is "no state", not an error.
    std::fs::write(check_path(&cache), "{not json").unwrap();
    assert!(read_check(&cache).is_none());
}

#[test]
fn only_the_exact_value_turns_the_notice_off() {
    assert!(disabled_by(Some("1")));
    for value in [None, Some(""), Some("0"), Some("true"), Some("yes")] {
        assert!(!disabled_by(value), "{value:?}");
    }
}

#[test]
fn the_notice_names_both_versions_and_is_silent_when_there_is_nothing_to_say() {
    let line = notice_line("v9.9.9", "0.1.0").expect("a newer release");
    assert!(line.contains("chap-devops v9.9.9 is available"), "{line}");
    assert!(line.contains("you have v0.1.0"), "{line}");
    assert!(line.contains("chap-devops self update"), "{line}");

    assert_eq!(notice_line("v0.1.0", "0.1.0"), None);
    assert_eq!(notice_line("v0.0.9", "0.1.0"), None);
    assert_eq!(notice_line("", "0.1.0"), None);
    assert_eq!(notice_line("latest", "0.1.0"), None);
}

// ----------------------------------------------------------------------
// Channels
// ----------------------------------------------------------------------

/// A dev release as the workflow publishes it: the `dev` tag, the notes
/// that name the commit, and the same asset names a tag release carries.
fn dev_release(commit: &str) -> Release {
    Release {
        tag: DEV_TAG.to_string(),
        assets: vec![
            "chap-devops-x86_64-unknown-linux-musl.tar.gz".to_string(),
            SUMS_FILE.to_string(),
        ],
        prerelease: true,
        published_at: "2026-09-23T17:08:44Z".to_string(),
        built_at: String::new(),
        body: format!(
            "### Unstable\n\nThis is a rolling build of `main`, \
                 built from commit `{commit}` on 2026-09-23.\n"
        ),
    }
}

#[test]
fn only_dev_names_the_dev_channel() {
    assert_eq!(Channel::from_name("dev"), Channel::Dev);
    assert_eq!(Channel::from_name(" DEV \n"), Channel::Dev);
    for other in ["stable", "", "development", "main", "nightly"] {
        assert_eq!(Channel::from_name(other), Channel::Stable, "{other}");
    }
    assert_eq!(Channel::Dev.as_str(), "dev");
    assert_eq!(Channel::Stable.as_str(), "stable");
    assert_eq!(Channel::Dev.tag(), Some(DEV_TAG));
    assert_eq!(Channel::Stable.tag(), None);
    // This build is one or the other, and a plain `cargo build` is stable.
    assert_eq!(channel(), Channel::from_name(BUILD_CHANNEL));
    assert_eq!(channel(), Channel::Stable, "an untagged build is stable");
}

#[test]
fn the_dev_release_says_which_commit_it_was_built_from() {
    let release = dev_release("0b1c2d3e4f50617283940a1b2c3d4e5f60718293");
    assert_eq!(
        release_commit(&release).as_deref(),
        Some("0b1c2d3e4f50617283940a1b2c3d4e5f60718293")
    );
    assert_eq!(release.built_day(), "2026-09-23");

    // A tagged release says nothing of the sort.
    let stable = Release {
        tag: "v0.2.1".to_string(),
        body: "### Downloads\n\nNothing about a commit here.\n".to_string(),
        ..Default::default()
    };
    assert_eq!(release_commit(&stable), None);

    // The word has to be followed by something that could be a commit.
    let vague = Release {
        body: "built from commit unknown".to_string(),
        ..Default::default()
    };
    assert_eq!(release_commit(&vague), None);

    // Upper case and a short revision are both accepted, and the answer
    // is normalised.
    let shouty = Release {
        body: "Commit ABC1234 it is".to_string(),
        ..Default::default()
    };
    assert_eq!(release_commit(&shouty).as_deref(), Some("abc1234"));
}

/// The commit is read out of notes this repository writes, so the reader
/// and the writer are checked against each other rather than only
/// against a fixture: first that `scripts/release-notes.sh` still writes
/// the line, then, where the script can run, that `release_commit` reads
/// the real script's output back as the commit this checkout is on. A
/// packaged crate without the script has nothing to check and says
/// nothing, and neither does Windows or a checkout too shallow for the
/// script to walk, where the first half is the whole test.
#[test]
fn the_notes_script_writes_the_line_the_commit_is_read_from() {
    let root = env!("CARGO_MANIFEST_DIR");
    let script = format!("{root}/scripts/release-notes.sh");
    let Ok(source) = std::fs::read_to_string(&script) else {
        return;
    };

    // `Built from commit` followed by a backtick, escaped for the heredoc
    // it sits in, and a shell expansion of the sha: `${GITHUB_SHA}`,
    // `${sha}` or `$sha`. The name has to mention the sha, so a rewrite
    // that interpolates something else fails here rather than in the
    // field.
    let expansion = source.lines().find_map(|line| {
        let rest = line.split_once("Built from commit")?.1;
        let name = rest
            .trim_start_matches([' ', '\\', '`'])
            .strip_prefix('$')?;
        let name: String = name
            .strip_prefix('{')
            .unwrap_or(name)
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        name.to_ascii_lowercase().contains("sha").then_some(name)
    });
    assert!(
        expansion.is_some(),
        "scripts/release-notes.sh no longer writes `Built from commit \
             <sha>` into the dev release notes, which is the line \
             release_commit reads"
    );

    // Git Bash on the windows-latest runner cannot run the script
    // reliably, so there the static check above is the guard.
    #[cfg(windows)]
    return;

    // The writer itself, where it can run: the notes the script prints
    // for the rolling build name the commit HEAD is on, and that is what
    // comes back out. No git, or no shell to run the script with, and
    // the line above is all that was checked.
    #[cfg(not(windows))]
    {
        let Some(head) = git(root, &["rev-parse", "HEAD"]) else {
            return;
        };

        // The notes are the commits since the previous version tag, so a
        // checkout without one, a shallow CI clone among them, has no
        // history for the script to read and nothing to check here.
        if git(root, &["tag", "-l", "v*"]).is_none() {
            return;
        }

        let Ok(printed) = std::process::Command::new("bash")
            .arg(&script)
            .arg("dev")
            // GITHUB_SHA is what the workflow builds from; here the
            // commit is whatever this checkout has, which is what HEAD
            // was read as.
            .env_remove("GITHUB_SHA")
            .output()
        else {
            return;
        };
        if !printed.status.success() {
            let stderr = String::from_utf8_lossy(&printed.stderr);
            let shallow = git(root, &["rev-parse", "--is-shallow-repository"])
                .is_some_and(|answer| answer == "true");
            // A failure with nothing to say, in a clone whose history is
            // cut short, is the missing history rather than the script.
            // Anything else is the script, and is a failure.
            assert!(
                stderr.trim().is_empty() && shallow,
                "scripts/release-notes.sh dev failed: {stderr}"
            );
            return;
        }

        let notes = Release {
            tag: DEV_TAG.to_string(),
            body: String::from_utf8_lossy(&printed.stdout).into_owned(),
            ..Default::default()
        };
        assert_eq!(
            release_commit(&notes).as_deref(),
            Some(head.to_ascii_lowercase().as_str()),
            "release_commit did not read HEAD out of the notes the script \
                 printed:\n{}",
            notes.body
        );
    }
}

/// One git command in this checkout, or `None` where git cannot answer,
/// which is a test that skips rather than one that fails. Only the live
/// half asks git anything, and that half does not run on Windows.
#[cfg(not(windows))]
fn git(root: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

#[test]
fn two_commits_match_on_the_shorter_of_the_two() {
    let full = "0b1c2d3e4f50617283940a1b2c3d4e5f60718293";
    assert!(same_commit(full, "0b1c2d3"));
    assert!(same_commit("0b1c2d3", full));
    assert!(same_commit("0B1C2D3", full));
    assert!(!same_commit(full, "deadbee"));
    // Too short to be anything, or not hexadecimal at all.
    assert!(!same_commit(full, "0b1c2d"));
    assert!(!same_commit(full, ""));
    assert!(!same_commit(full, "not-a-sha"));
}

#[test]
fn a_dev_build_is_up_to_date_only_on_the_commit_the_release_names() {
    let commit = "0b1c2d3e4f50617283940a1b2c3d4e5f60718293";
    let release = dev_release(commit);

    assert!(!dev_update_available(&release, "0b1c2d3"));
    assert!(dev_update_available(&release, "deadbee"));
    // A build with no revision, and a release with no commit in its
    // notes, both mean "cannot tell", and an update that was asked for
    // goes ahead rather than refusing.
    assert!(dev_update_available(&release, ""));
    assert!(dev_update_available(
        &Release {
            tag: DEV_TAG.to_string(),
            ..Default::default()
        },
        "0b1c2d3"
    ));
}

#[test]
fn the_dev_notice_speaks_only_when_it_knows_the_commit_moved() {
    let commit = "0b1c2d3e4f50617283940a1b2c3d4e5f60718293";
    let line = dev_notice_line(commit, "deadbee").expect("a different commit");
    assert!(line.contains("newer chap-devops dev build"), "{line}");
    assert!(line.contains("commit 0b1c2d3"), "{line}");
    assert!(line.contains("you have deadbee"), "{line}");
    assert!(line.contains("chap-devops self update"), "{line}");

    // The same build, and the two "cannot tell" cases, say nothing: this
    // line is printed without being asked for.
    assert_eq!(dev_notice_line(commit, "0b1c2d3"), None);
    assert_eq!(dev_notice_line(commit, ""), None);
    assert_eq!(dev_notice_line("", "deadbee"), None);
    assert_eq!(dev_notice_line("latest", "deadbee"), None);
}

/// The stable notice is fed from `releases/latest`, which GitHub
/// documents as excluding pre-releases, so the `dev` tag can never reach
/// it. The tag is not a version either, so even if it did, nothing would
/// be printed.
#[test]
fn the_stable_notice_cannot_be_talked_into_offering_a_prerelease() {
    assert_eq!(notice_line(DEV_TAG, VERSION), None);
    assert!(!is_newer_than_current(DEV_TAG));

    let parsed = parse_release(
        r#"{"tag_name":"dev","prerelease":true,"published_at":"2026-09-23T17:08:44Z",
                "body":"built from commit abc1234 on 2026-09-23",
                "assets":[{"name":"chap-devops-x86_64-unknown-linux-musl.tar.gz"}]}"#,
    )
    .unwrap();
    assert!(parsed.prerelease);
    assert_eq!(parsed.tag, DEV_TAG);
    assert_eq!(release_commit(&parsed).as_deref(), Some("abc1234"));
    assert_eq!(parsed.published_at, "2026-09-23T17:08:44Z");

    // A tagged release parses as what it is, with nulls tolerated.
    let stable = parse_release(r#"{"tag_name":"v0.2.1","published_at":null,"body":null}"#).unwrap();
    assert!(!stable.prerelease);
    assert!(stable.published_at.is_empty());
    assert_eq!(release_commit(&stable), None);
}

#[test]
fn the_check_state_carries_the_commit() {
    let tmp = tempfile::tempdir().unwrap();
    write_check(
        tmp.path(),
        &CheckState {
            checked_at_unix: 1_700_000_000,
            latest: DEV_TAG.to_string(),
            commit: "0b1c2d3".to_string(),
        },
    );
    let state = read_check(tmp.path()).expect("written a moment ago");
    assert_eq!(state.latest, DEV_TAG);
    assert_eq!(state.commit, "0b1c2d3");
}

#[test]
fn the_download_urls_name_the_repository_and_the_tag() {
    assert_eq!(
        download_base("v0.2.0"),
        "https://github.com/dhis2-chap/chap-devops/releases/download/v0.2.0"
    );
    assert!(latest_release_url().contains(REPO));
    assert!(latest_release_url().starts_with(crate::github::DEFAULT_API));
    assert!(release_url("v0.2.0").ends_with("/releases/tags/v0.2.0"));
}

/// A port nothing listens on covers the transport-error path without a
/// network.
#[test]
fn an_unreachable_host_is_an_error_not_a_hang() {
    let base = crate::test_support::unreachable_base();
    let err = get_text(&format!("{base}/releases/latest"), Duration::from_secs(2))
        .expect_err("nothing is listening there");
    assert!(err.to_string().contains(&base[7..]), "{err}");
}

#[test]
fn a_status_code_becomes_a_typed_http_error() {
    let feed = latest_release_url();
    let err = map_error(&feed, ureq::Error::StatusCode(403));
    match err.downcast_ref::<ChapError>() {
        Some(ChapError::Http { url, status }) => {
            assert_eq!(*url, feed);
            assert_eq!(*status, 403);
        }
        other => panic!("wrong error: {other:?}"),
    }
}

// ----------------------------------------------------------------------
// A release served off a local socket
//
// The whole path an update takes - look up the release, download the
// archive and the manifest, verify, unpack, swap - against a server in a
// thread, so nothing here needs GitHub or the network.
// ----------------------------------------------------------------------

/// Serve fixed bodies by path on a loopback port, for as long as the test
/// binary runs.
fn serve(routes: Vec<(String, Vec<u8>)>) -> String {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let base = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let Ok(peek) = stream.try_clone() else {
                continue;
            };
            let mut reader = BufReader::new(peek);

            let mut request = String::new();
            if reader.read_line(&mut request).is_err() {
                continue;
            }
            // Drain the headers so the client is not left writing into a
            // socket nobody reads.
            loop {
                let mut header = String::new();
                match reader.read_line(&mut header) {
                    Ok(0) => break,
                    Ok(_) if header == "\r\n" || header == "\n" => break,
                    Ok(_) => continue,
                    Err(_) => break,
                }
            }

            let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
            let (status, body) = match routes.iter().find(|(route, _)| *route == path) {
                Some((_, body)) => ("200 OK", body.clone()),
                None => ("404 Not Found", b"no such asset".to_vec()),
            };
            // `Connection: close` keeps each request independent, which
            // is what makes a server this small enough.
            let head = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\n\
                     Content-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
            let _ = stream.flush();
        }
    });
    base
}

const FEED: &str = r#"{
      "tag_name": "v9.9.9",
      "assets": [
        {"name": "chap-devops-x86_64-unknown-linux-musl.tar.gz"},
        {"name": "SHA256SUMS"}
      ]
    }"#;

#[test]
fn a_release_feed_is_read_over_http() {
    let base = serve(vec![
        ("/releases/latest".to_string(), FEED.as_bytes().to_vec()),
        (
            "/releases/tags/v9.9.9".to_string(),
            FEED.as_bytes().to_vec(),
        ),
    ]);

    let release = release_at(&format!("{base}/releases/latest"), Duration::from_secs(5))
        .expect("the feed parses");
    assert_eq!(release.tag, "v9.9.9");
    assert_eq!(
        pick_asset(&release, LINUX).unwrap(),
        "chap-devops-x86_64-unknown-linux-musl.tar.gz"
    );
    assert!(is_newer_than_current(&release.tag));

    // A tag that is not there is a 404, and a 404 is a typed error rather
    // than a parse failure on an HTML page.
    let err = release_at(
        &format!("{base}/releases/tags/v0.0.1"),
        Duration::from_secs(5),
    )
    .expect_err("no such tag");
    match err.downcast_ref::<ChapError>() {
        Some(ChapError::Http { status, .. }) => assert_eq!(*status, 404),
        other => panic!("wrong error: {other:?}"),
    }
}

#[test]
fn an_archive_is_downloaded_verified_unpacked_and_swapped_in() {
    let tmp = tempfile::tempdir().unwrap();

    // Build an archive shaped like a release: the asset is named after
    // the target alone, and the one directory inside it carries the tag.
    let tag = "v9.9.9";
    let name = format!("chap-devops-{tag}-{}", asset_target(TARGET));
    let staging = tmp.path().join("staging");
    std::fs::create_dir_all(staging.join(&name).join("completions")).unwrap();
    std::fs::write(
        staging.join(&name).join(binary_name(TARGET)),
        b"the new binary",
    )
    .unwrap();
    std::fs::write(staging.join(&name).join("README.md"), b"readme").unwrap();
    std::fs::write(
        staging
            .join(&name)
            .join("completions")
            .join("chap-devops.bash"),
        b"completions",
    )
    .unwrap();

    let asset = asset_name(TARGET);
    let archive = tmp.path().join(&asset);
    let packed = std::process::Command::new("tar")
        .arg("-czf")
        .arg(&archive)
        .arg("-C")
        .arg(&staging)
        .arg(&name)
        .status()
        .expect("tar runs");
    assert!(packed.success(), "tar packed the fixture");

    let bytes = std::fs::read(&archive).unwrap();
    let sums = format!("{}  {asset}\n", sha256_hex(&bytes));
    let base = serve(vec![
        (format!("/{asset}"), bytes.clone()),
        (format!("/{SUMS_FILE}"), sums.into_bytes()),
    ]);

    // What `self update` does, in the order it does it.
    let downloaded =
        download(&format!("{base}/{asset}"), Duration::from_secs(10)).expect("the archive");
    assert_eq!(downloaded, bytes);
    let manifest = download_text(&format!("{base}/{SUMS_FILE}"), Duration::from_secs(10))
        .expect("the manifest");
    verify(&manifest, &asset, &downloaded).expect("the archive matches the manifest");

    let unpacked = tmp.path().join("unpacked");
    std::fs::create_dir_all(&unpacked).unwrap();
    let staged_archive = unpacked.join(&asset);
    std::fs::write(&staged_archive, &downloaded).unwrap();
    extract(&staged_archive, &unpacked).expect("tar unpacks it");

    let found = find_binary(&unpacked, TARGET).expect("the binary is in there");
    assert_eq!(std::fs::read(&found).unwrap(), b"the new binary");

    // And the swap, on a copy that stands in for the installed binary.
    let installed = tmp.path().join("bin").join(binary_name(TARGET));
    std::fs::create_dir_all(installed.parent().unwrap()).unwrap();
    std::fs::write(&installed, b"the old binary").unwrap();
    #[cfg(unix)]
    set_mode(&installed, 0o755);
    replace_executable(&installed, &std::fs::read(&found).unwrap()).expect("the swap");
    assert_eq!(std::fs::read(&installed).unwrap(), b"the new binary");
    #[cfg(unix)]
    assert_eq!(mode_of(&installed), 0o755);

    // A manifest that does not agree with the bytes stops the update
    // before anything is written.
    let tampered = format!("{}  {asset}\n", sha256_hex(b"something else"));
    let err = verify(&tampered, &asset, &downloaded).expect_err("the digests differ");
    assert!(err.to_string().contains("does not match"), "{err}");
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[cfg(unix)]
fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o7777
}

/// The rolling release keeps the day it was first published; the day of the
/// build is in its notes, and in the assets each build replaces.
#[test]
fn the_build_day_is_the_day_of_the_build_not_of_the_release() {
    let parsed = parse_release(
        r#"{"tag_name":"dev","prerelease":true,"published_at":"2026-09-23T17:08:44Z",
                "body":"Built from commit `b95828a0` on 2026-10-07 06:12 UTC.",
                "assets":[{"name":"SHA256SUMS","updated_at":"2026-10-07T06:20:00Z"},
                          {"name":"chap-devops-x86_64-unknown-linux-musl.tar.gz",
                           "updated_at":"2026-10-07T06:19:00Z"}]}"#,
    )
    .unwrap();
    assert_eq!(parsed.built_at, "2026-10-07T06:20:00Z");
    assert_eq!(parsed.built_day(), "2026-10-07");

    // Notes without a day: the newest asset says when the build was.
    let assets_only = Release {
        body: "Built from commit `b95828a0`.".to_string(),
        built_at: "2026-10-06T06:20:00Z".to_string(),
        ..parsed.clone()
    };
    assert_eq!(assets_only.built_day(), "2026-10-06");

    // Nothing at all: no day, rather than the day the release was made.
    let bare = Release {
        body: String::new(),
        built_at: String::new(),
        ..parsed
    };
    assert_eq!(bare.built_day(), "");
}
