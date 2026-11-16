//! The model projects that `chap-devops models new` creates.
//!
//! Two kinds, each on one of the chapkit base images:
//!
//! - a chapkit service: `main.py` builds the service with chapkit, and the
//!   model is in `main.py` (`fn-py`) or in scripts that the service runs
//!   (`shell-*`);
//! - an MLproject: an `MLproject` file and scripts, which the image serves
//!   with `chapkit mlproject run`. The project has no chapkit code.
//!
//! The templates are embedded in the binary and rendered with minijinja, so a
//! new project needs neither Python nor the chapkit CLI on the machine. Every
//! type has the same example model, so that the docs show one example.

use crate::error::Result;
use anyhow::{Context, bail};
use minijinja::Environment;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// The chapkit versions a new chapkit service depends on: the release the
/// templates were checked against, up to the next major version.
pub const CHAPKIT_REQUIREMENT: &str = "chapkit>=2.4.1,<3";

/// Where the chapkit base images are published.
const IMAGE_PREFIX: &str = "ghcr.io/dhis2-chap/";

/// The kind of project, and the base image it builds on. The values have
/// no help text of their own: the table in `docs/models.md` explains them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Template {
    // chapkit service, the model in Python in main.py
    FnPy,
    // chapkit service, the model in Python scripts
    ShellPy,
    // chapkit service, the model in R scripts on plain R
    ShellR,
    // chapkit service, R scripts with tidyverse and fable
    ShellRTidyverse,
    // chapkit service, R scripts with INLA (amd64 only)
    ShellRInla,
    // MLproject, the model in Python scripts
    MlprojectPy,
    // MLproject, the model in R scripts on plain R
    MlprojectR,
    // MLproject, R scripts with tidyverse and fable
    MlprojectRTidyverse,
    // MLproject, R scripts with INLA (amd64 only)
    MlprojectRInla,
}

/// The language and packages of a base image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Base {
    Py,
    R,
    RTidyverse,
    RInla,
}

impl Template {
    /// The spelling of `--template`.
    pub fn as_str(self) -> &'static str {
        match self {
            Template::FnPy => "fn-py",
            Template::ShellPy => "shell-py",
            Template::ShellR => "shell-r",
            Template::ShellRTidyverse => "shell-r-tidyverse",
            Template::ShellRInla => "shell-r-inla",
            Template::MlprojectPy => "mlproject-py",
            Template::MlprojectR => "mlproject-r",
            Template::MlprojectRTidyverse => "mlproject-r-tidyverse",
            Template::MlprojectRInla => "mlproject-r-inla",
        }
    }

    pub fn is_mlproject(self) -> bool {
        matches!(
            self,
            Template::MlprojectPy
                | Template::MlprojectR
                | Template::MlprojectRTidyverse
                | Template::MlprojectRInla
        )
    }

    fn base(self) -> Base {
        match self {
            Template::FnPy | Template::ShellPy | Template::MlprojectPy => Base::Py,
            Template::ShellR | Template::MlprojectR => Base::R,
            Template::ShellRTidyverse | Template::MlprojectRTidyverse => Base::RTidyverse,
            Template::ShellRInla | Template::MlprojectRInla => Base::RInla,
        }
    }

    fn is_r(self) -> bool {
        self.base() != Base::Py
    }

    /// Whether the model is in scripts: every type but `fn-py`.
    fn has_scripts(self) -> bool {
        self != Template::FnPy
    }

    /// INLA has x86_64 builds only, so its base image is amd64 only.
    pub fn amd64_only(self) -> bool {
        self.base() == Base::RInla
    }

    /// The base image. A chapkit service installs its own chapkit with
    /// `uv sync`; an MLproject uses the `-cli` image, which has chapkit. A
    /// Python MLproject uses `chapkit-py-data-cli`, which also has numpy,
    /// pandas and the rest of the PyData stack, so it runs with no install.
    pub fn base_image(self) -> String {
        let name = match self.base() {
            Base::Py if self.is_mlproject() => "chapkit-py-data",
            Base::Py => "chapkit-py",
            Base::R => "chapkit-r",
            Base::RTidyverse => "chapkit-r-tidyverse",
            Base::RInla => "chapkit-r-inla",
        };
        let cli = if self.is_mlproject() { "-cli" } else { "" };
        format!("{IMAGE_PREFIX}{name}{cli}:latest")
    }
}

/// The author that a new project names in its model metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Author {
    pub name: String,
    pub email: String,
    /// Whether the values come from git, and not from the placeholders.
    #[serde(skip)]
    pub from_git: bool,
}

/// The author name when git has no `user.name`.
pub const PLACEHOLDER_NAME: &str = "Your Name";
/// The contact email when git has no `user.email`.
pub const PLACEHOLDER_EMAIL: &str = "your.email@example.com";

impl Default for Author {
    fn default() -> Self {
        Self {
            name: PLACEHOLDER_NAME.to_string(),
            email: PLACEHOLDER_EMAIL.to_string(),
            from_git: false,
        }
    }
}

impl Author {
    /// The `user.name` and `user.email` of git, each replaced by its
    /// placeholder when git has no value or is not installed.
    pub fn from_git() -> Self {
        Self::from_values(git_config("user.name"), git_config("user.email"))
    }

    /// An author from the two git values; a missing one keeps its placeholder.
    pub fn from_values(name: Option<String>, email: Option<String>) -> Self {
        let name = name
            .map(|value| clean(&value))
            .filter(|value| !value.is_empty());
        let email = email
            .map(|value| clean(&value))
            .filter(|value| !value.is_empty());
        let placeholder = Self::default();
        Self {
            from_git: name.is_some() || email.is_some(),
            name: name.unwrap_or(placeholder.name),
            email: email.unwrap_or(placeholder.email),
        }
    }
}

/// One value of `git config`, or `None` when it is not set or git is missing.
fn git_config(key: &str) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["config", "--get", key])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// The value without the characters that would end a quoted string in the
/// templates: quotes, backslashes and control characters.
fn clean(value: &str) -> String {
    value
        .chars()
        .filter(|c| !matches!(c, '"' | '\\') && !c.is_control())
        .collect::<String>()
        .trim()
        .to_string()
}

/// The names a project gets from its directory name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Names {
    /// The display name: the directory name as it was given.
    pub name: String,
    /// The id the service registers with chap-core under, and the image name.
    pub service_id: String,
    /// The chap-devops model id and the Python package name.
    pub model_id: String,
    /// The prefix of the config class in `main.py`.
    #[serde(skip)]
    pub class_name: String,
}

/// Make the names of a project from its directory name.
///
/// The service id is lowercase letters, digits and `-`, and starts with a
/// letter, which is what chapkit accepts. The model id is the same with `_`,
/// which is the id `chap-devops models add` gives the image.
pub fn names(dir_name: &str) -> Result<Names> {
    let name = dir_name.trim();
    if let Some(bad) = name
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.')))
    {
        bail!(
            "the name `{name}` has the character `{bad}`; use letters, digits, `-` and `_` in the \
             directory name"
        );
    }
    let mut service_id = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            service_id.push(c.to_ascii_lowercase());
        } else if !service_id.is_empty() && !service_id.ends_with('-') {
            service_id.push('-');
        }
    }
    let service_id = service_id.trim_end_matches('-').to_string();
    if service_id.is_empty() {
        bail!("the name `{name}` has no letters or digits; give the directory a name");
    }
    let service_id = if service_id.starts_with(|c: char| c.is_ascii_digit()) {
        format!("model-{service_id}")
    } else {
        service_id
    };
    let model_id = service_id.replace('-', "_");
    let class_name = model_id
        .split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect();
    Ok(Names {
        name: name.to_string(),
        service_id,
        model_id,
        class_name,
    })
}

/// The embedded templates, by name. A name that ends in `.jinja` is
/// rendered; the others are copied as they are.
const SOURCES: &[(&str, &str)] = &[
    (
        "service/main_fn.py.jinja",
        include_str!("scaffold/templates/service/main_fn.py.jinja"),
    ),
    (
        "service/main_shell.py.jinja",
        include_str!("scaffold/templates/service/main_shell.py.jinja"),
    ),
    (
        "service/pyproject.toml.jinja",
        include_str!("scaffold/templates/service/pyproject.toml.jinja"),
    ),
    (
        "service/Dockerfile.jinja",
        include_str!("scaffold/templates/service/Dockerfile.jinja"),
    ),
    (
        "mlproject/MLproject.jinja",
        include_str!("scaffold/templates/mlproject/MLproject.jinja"),
    ),
    (
        "mlproject/Dockerfile.jinja",
        include_str!("scaffold/templates/mlproject/Dockerfile.jinja"),
    ),
    (
        "mlproject/pyproject.toml.jinja",
        include_str!("scaffold/templates/mlproject/pyproject.toml.jinja"),
    ),
    (
        "scripts/train.py",
        include_str!("scaffold/templates/scripts/train.py"),
    ),
    (
        "scripts/predict.py",
        include_str!("scaffold/templates/scripts/predict.py"),
    ),
    (
        "scripts/train.R",
        include_str!("scaffold/templates/scripts/train.R"),
    ),
    (
        "scripts/predict.R",
        include_str!("scaffold/templates/scripts/predict.R"),
    ),
    (
        "common/README.md.jinja",
        include_str!("scaffold/templates/common/README.md.jinja"),
    ),
    (
        "common/publish.yml.jinja",
        include_str!("scaffold/templates/common/publish.yml.jinja"),
    ),
    (
        "common/gitignore",
        include_str!("scaffold/templates/common/gitignore"),
    ),
    (
        "common/dockerignore",
        include_str!("scaffold/templates/common/dockerignore"),
    ),
    (
        "common/python-version",
        include_str!("scaffold/templates/common/python-version"),
    ),
];

/// The files of a project: the path in the project, and the template.
pub fn plan(template: Template) -> Vec<(&'static str, &'static str)> {
    let mut files = Vec::new();
    if template.is_mlproject() {
        files.push(("MLproject", "mlproject/MLproject.jinja"));
        files.push(("Dockerfile", "mlproject/Dockerfile.jinja"));
        if !template.is_r() {
            files.push(("pyproject.toml", "mlproject/pyproject.toml.jinja"));
        }
    } else {
        let main = if template.has_scripts() {
            "service/main_shell.py.jinja"
        } else {
            "service/main_fn.py.jinja"
        };
        files.push(("main.py", main));
        files.push(("pyproject.toml", "service/pyproject.toml.jinja"));
        files.push(("Dockerfile", "service/Dockerfile.jinja"));
    }
    if template.has_scripts() {
        if template.is_r() {
            files.push(("scripts/train.R", "scripts/train.R"));
            files.push(("scripts/predict.R", "scripts/predict.R"));
        } else {
            files.push(("scripts/train.py", "scripts/train.py"));
            files.push(("scripts/predict.py", "scripts/predict.py"));
        }
    }
    files.push(("README.md", "common/README.md.jinja"));
    files.push((".github/workflows/publish.yml", "common/publish.yml.jinja"));
    files.push((".gitignore", "common/gitignore"));
    files.push((".dockerignore", "common/dockerignore"));
    if files.iter().any(|(path, _)| *path == "pyproject.toml") {
        files.push((".python-version", "common/python-version"));
    }
    files
}

/// The values the templates read.
fn context(
    names: &Names,
    author: &Author,
    template: Template,
    with_validation: bool,
) -> minijinja::Value {
    let is_r = template.is_r();
    minijinja::context! {
        author => author.name.clone(),
        contact_email => author.email.clone(),
        name => names.name.clone(),
        service_id => names.service_id.clone(),
        model_id => names.model_id.clone(),
        slug => names.model_id.clone(),
        class_name => names.class_name.clone(),
        template => template.as_str(),
        with_validation => with_validation,
        is_r => is_r,
        is_shell => template.has_scripts() && !template.is_mlproject(),
        is_mlproject => template.is_mlproject(),
        has_pyproject => !(template.is_mlproject() && is_r),
        amd64_only => template.amd64_only(),
        base_image => template.base_image(),
        chapkit_requirement => CHAPKIT_REQUIREMENT,
        language => if is_r { "R" } else { "Python" },
        run => if is_r { "Rscript" } else { "python" },
        train_script => if is_r { "train.R" } else { "train.py" },
        predict_script => if is_r { "predict.R" } else { "predict.py" },
        model_file => if is_r { "model.rds" } else { "model.pickle" },
    }
}

/// Render every file of a project, in memory: the path in the project and
/// the content.
pub fn render(
    names: &Names,
    author: &Author,
    template: Template,
    with_validation: bool,
) -> Result<Vec<(&'static str, String)>> {
    let mut env = Environment::new();
    env.set_syntax(crate::compose::render::block_syntax());
    for (name, source) in SOURCES {
        env.add_template(name, source)
            .with_context(|| format!("the embedded template {name} does not parse"))?;
    }
    let ctx = context(names, author, template, with_validation);
    plan(template)
        .into_iter()
        .map(|(path, source)| {
            let body = if source.ends_with(".jinja") {
                env.get_template(source)?
                    .render(&ctx)
                    .with_context(|| format!("could not render the template {source}"))?
            } else {
                SOURCES
                    .iter()
                    .find(|(name, _)| *name == source)
                    .map(|(_, body)| body.to_string())
                    .with_context(|| format!("the embedded file {source} is missing"))?
            };
            Ok((path, body))
        })
        .collect()
}

/// Write the files into `dir`, which must be missing or empty. Returns the
/// paths that were written, under `dir`.
pub fn write(dir: &Path, files: &[(&str, String)]) -> Result<Vec<PathBuf>> {
    if dir.exists() {
        let mut entries =
            std::fs::read_dir(dir).with_context(|| format!("could not read {}", dir.display()))?;
        if entries.next().is_some() {
            bail!(
                "{} already exists and is not empty; give a new directory",
                dir.display()
            );
        }
    }
    let mut written = Vec::new();
    for (path, body) in files {
        let target = dir.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("could not make {}", parent.display()))?;
        }
        std::fs::write(&target, body)
            .with_context(|| format!("could not write {}", target.display()))?;
        written.push(target);
    }
    Ok(written)
}

#[cfg(test)]
mod tests;
