//! The images that serve an MLproject: the short names `--image` takes,
//! their list, the default for a train command, and the pull.

use crate::commands::Ctx;
use crate::docker;
use crate::error::{ChapError, Result};
use crate::output;
use serde::Serialize;

/// Where the chapkit images are published.
const IMAGE_PREFIX: &str = "ghcr.io/dhis2-chap/";

/// A short name that `--image` takes, and the chapkit `-cli` image it means.
pub(super) struct Flavour {
    pub name: &'static str,
    pub image: &'static str,
    /// What the image has, besides chapkit.
    pub has: &'static str,
}

/// The short names `--image` takes.
pub(super) const FLAVOURS: &[Flavour] = &[
    Flavour {
        name: "py",
        image: "chapkit-py-cli",
        has: "Python",
    },
    Flavour {
        name: "py-data",
        image: "chapkit-py-data-cli",
        has: "Python, numpy, pandas, scipy, scikit-learn, statsmodels, ...",
    },
    Flavour {
        name: "r",
        image: "chapkit-r-cli",
        has: "R, renv, pak",
    },
    Flavour {
        name: "r-tidyverse",
        image: "chapkit-r-tidyverse-cli",
        has: "R, tidyverse, fable, tsibble, forecast, xgboost, ...",
    },
    Flavour {
        name: "r-inla",
        image: "chapkit-r-inla-cli",
        has: "R, tidyverse, INLA, sf, spdep, dlnm, ...",
    },
];

/// One row of `chap-devops chapkit --image`, and of its `--json`.
#[derive(Debug, Serialize)]
struct ImageRow {
    name: &'static str,
    image: String,
    has: &'static str,
    platforms: &'static str,
    /// Whether this docker has the image, so a run does not pull it.
    local: bool,
}

/// What `chap-devops chapkit --image` prints.
#[derive(Debug, Serialize)]
struct ImageList {
    images: Vec<ImageRow>,
}

/// List the images that `--image` takes by short name.
pub(in crate::commands::chapkit) fn list(ctx: &Ctx) -> Result<()> {
    let images: Vec<ImageRow> = FLAVOURS
        .iter()
        .map(|flavour| {
            let image = image_for(flavour.name);
            ImageRow {
                name: flavour.name,
                platforms: match amd64_only(&image) {
                    true => "amd64",
                    false => "amd64, arm64",
                },
                local: docker::image_is_local(&image),
                has: flavour.has,
                image,
            }
        })
        .collect();
    if !ctx.out.json {
        let rows: Vec<Vec<String>> = images
            .iter()
            .map(|row| {
                vec![
                    row.name.to_string(),
                    row.image.clone(),
                    row.platforms.to_string(),
                    match row.local {
                        true => ctx.out.ok("yes"),
                        false => ctx.out.dim("no"),
                    },
                    ctx.out.dim(row.has),
                ]
            })
            .collect();
        print!(
            "{}",
            ctx.out.table(
                &["NAME", "IMAGE", "PLATFORMS", "LOCAL", "WHAT IT HAS"],
                &rows
            )
        );
    }
    let list = ImageList { images };
    ctx.out.report(&list, |r| {
        r.info(
            "`chap-devops chapkit --image NAME mlproject run DIR` serves an MLproject in one; \
             an image name works too",
        );
        r.hint(
            "without --image, the train command selects one: `python` or `uv` gets py-data, \
             `Rscript` gets r-tidyverse",
        );
    })
}

/// The image for `--image`: a short name maps to its chapkit `-cli` image,
/// anything else is an image name.
pub(super) fn image_for(choice: &str) -> String {
    match FLAVOURS.iter().find(|flavour| flavour.name == choice) {
        Some(flavour) => format!("{IMAGE_PREFIX}{}:latest", flavour.image),
        None => choice.to_string(),
    }
}

/// The short name for the language of the train command, and the command:
/// `py-data` for Python, `r-tidyverse` for R. `None` when the MLproject has no
/// train command or the command is in another language.
pub(super) fn default_flavour(mlproject: &str) -> Option<(&'static str, String)> {
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(mlproject).ok()?;
    let command = value["entry_points"]["train"]["command"]
        .as_str()?
        .trim()
        .to_string();
    let program = command.split_whitespace().next()?.rsplit('/').next()?;
    let flavour = match program {
        "python" | "python3" | "uv" | "uvx" => "py-data",
        "Rscript" | "R" => "r-tidyverse",
        _ => return None,
    };
    Some((flavour, command))
}

/// Whether the image exists for amd64 only, so it runs pinned to amd64.
pub(super) fn amd64_only(image: &str) -> bool {
    image.contains("chapkit-r-inla")
}

/// Pull the image when this docker does not have it.
pub(super) fn ensure_image(image: &str) -> Result<()> {
    if docker::image_is_local(image) {
        return Ok(());
    }
    output::notice(&format!("pulling {image}, which this docker does not have"));
    let mut args: Vec<String> = vec!["pull".to_string()];
    if amd64_only(image) {
        args.extend(["--platform", crate::compose::AMD64_PLATFORM].map(str::to_string));
    }
    args.push(image.to_string());
    match docker::run_plain(&args)? {
        0 => Ok(()),
        _ => Err(ChapError::Usage(format!(
            "this docker does not have {image}, and could not pull it; check the name, or build \
             it first with `docker build -t {image} .`"
        ))
        .into()),
    }
}
