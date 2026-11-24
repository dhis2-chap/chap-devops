//! Which Modeling App build `chap-devops dhis2 apps` and `connect` install,
//! and which unstable build they installed last.

use super::*;

/// Which Modeling App build to install.
///
/// Written to `components.yaml` as `stable`, `unstable` or a version such as
/// `7.5.0`; `stable` is the default and is not written.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum ModelingApp {
    /// The newest App Hub version that the DHIS2 version can run.
    #[default]
    Stable,
    /// One App Hub version, such as `7.5.0`.
    Version(String),
    /// The latest build of chap-frontend's `main` branch, which chap-frontend
    /// publishes as the GitHub pre-release `latest`.
    Unstable,
}

impl ModelingApp {
    pub fn is_stable(&self) -> bool {
        *self == ModelingApp::Stable
    }

    /// `stable`, `unstable`, or a version of three numbers, with or without a
    /// leading `v`.
    pub fn parse(text: &str) -> std::result::Result<ModelingApp, String> {
        let text = text.trim();
        match text {
            "stable" => return Ok(ModelingApp::Stable),
            "unstable" => return Ok(ModelingApp::Unstable),
            _ => {}
        }
        let version = text.strip_prefix('v').unwrap_or(text);
        let parts: Vec<&str> = version.split('.').collect();
        let numbers = parts.len() == 3
            && parts
                .iter()
                .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()));
        match numbers {
            true => Ok(ModelingApp::Version(version.to_string())),
            false => Err(format!(
                "`{text}` is not a Modeling App build; give stable, unstable, or a version \
                 such as 7.5.0"
            )),
        }
    }
}

impl std::fmt::Display for ModelingApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelingApp::Stable => f.write_str("stable"),
            ModelingApp::Unstable => f.write_str("unstable"),
            ModelingApp::Version(version) => f.write_str(version),
        }
    }
}

impl TryFrom<String> for ModelingApp {
    type Error = String;
    fn try_from(text: String) -> std::result::Result<ModelingApp, String> {
        ModelingApp::parse(&text)
    }
}

impl From<ModelingApp> for String {
    fn from(which: ModelingApp) -> String {
        which.to_string()
    }
}

/// The unstable build `chap-devops` installed last.
///
/// DHIS2 cannot say which build it has: the unstable build carries the
/// version of the release before it, so the instance lists it as that
/// release. This record is how a later run knows whether `main` moved, and how
/// `chap-devops dhis2 show` names the build.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelingAppBuild {
    /// The chap-frontend commit the build is from.
    pub commit: String,
    /// The `sha256:` digest of the zip, which changes with every build.
    pub digest: String,
}

impl ModelingAppBuild {
    /// The first seven characters of the commit.
    pub fn short_commit(&self) -> &str {
        self.commit.get(..7).unwrap_or(&self.commit)
    }
}

impl Components {
    /// The Modeling App setting of whichever DHIS2 `chap-devops dhis2` talks
    /// to: the external one when there is one, the component otherwise.
    pub fn modeling_app(&self) -> &ModelingApp {
        match &self.dhis2_external {
            Some(external) => &external.modeling_app,
            None => &self.dhis2.modeling_app,
        }
    }

    pub fn modeling_app_mut(&mut self) -> &mut ModelingApp {
        match &mut self.dhis2_external {
            Some(external) => &mut external.modeling_app,
            None => &mut self.dhis2.modeling_app,
        }
    }

    /// The unstable build recorded for that same DHIS2, if one was installed.
    pub fn modeling_app_build(&self) -> Option<&ModelingAppBuild> {
        match &self.dhis2_external {
            Some(external) => external.modeling_app_build.as_ref(),
            None => self.dhis2.modeling_app_build.as_ref(),
        }
    }

    pub fn modeling_app_build_mut(&mut self) -> &mut Option<ModelingAppBuild> {
        match &mut self.dhis2_external {
            Some(external) => &mut external.modeling_app_build,
            None => &mut self.dhis2.modeling_app_build,
        }
    }
}
