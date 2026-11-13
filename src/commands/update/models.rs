//! The enabled models: re-resolved against the marketplace, or against their
//! own repositories for a manually added one.

use crate::compose::resolve::{self, UserSource};
use crate::error::{ChapError, Result};
use crate::manual;
use crate::project::{ManualModel, Project};
use crate::registry::{Registry, VersionSelector};
use serde::Serialize;

/// One enabled model's before and after.
#[derive(Debug, Clone, Serialize)]
pub struct ModelUpdate {
    pub id: String,
    pub old_version: String,
    pub old_tag: String,
    pub new_version: String,
    pub new_tag: String,
    /// The digest pinned with the tag before and after; see
    /// [`crate::compose::pinned_tag`].
    pub old_digest: Option<String>,
    pub new_digest: Option<String>,
    pub changed: bool,
    /// Pinned to an exact version, so nothing was consulted.
    pub pinned: bool,
    /// A model this deployment added itself, whose pin comes from a
    /// repository rather than from the marketplace.
    pub manual: bool,
    /// The branch a manual entry follows, `null` for every other row.
    pub follow: Option<String>,
    /// Whether the lookup behind this row could be made at all. False is a
    /// manual entry whose repository or registry would not answer, which is
    /// "unchanged" without the claim that nothing has moved.
    pub checked: bool,
    /// The commit the new tag was built from, where the lookup said.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_commit: Option<String>,
    /// The account the model runs as today.
    pub old_user: String,
    /// What the image at the new tag declares, which is the same string
    /// unless the image changed what it runs as between the two builds.
    pub new_user: String,
    /// Where [`new_user`] came from.
    ///
    /// [`new_user`]: ModelUpdate::new_user
    pub user_from: UserSource,
    /// Whether the image at the new tag reads its port from `PORT`.
    pub reads_port: bool,
    /// Whether the registry lists a `linux/arm64` image for the new tag too.
    #[serde(skip)]
    pub multi_arch: bool,
}

impl ModelUpdate {
    /// The pin this row moves to, with its digest.
    pub(super) fn new_pin(&self) -> String {
        crate::compose::pinned_tag(&self.new_tag, self.new_digest.as_deref())
    }

    /// The row this entry starts as: unchanged, and checked.
    pub(super) fn unchanged(id: &str, entry: &crate::project::EnabledModel) -> ModelUpdate {
        ModelUpdate {
            id: id.to_string(),
            old_version: entry.version.clone(),
            old_tag: entry.image_tag.clone(),
            new_version: entry.version.clone(),
            new_tag: entry.image_tag.clone(),
            old_digest: entry.image_digest.clone(),
            new_digest: entry.image_digest.clone(),
            changed: false,
            pinned: entry.channel.is_none(),
            manual: false,
            follow: None,
            checked: true,
            new_commit: None,
            old_user: entry.user.clone(),
            new_user: entry.user.clone(),
            user_from: entry.user_from,
            reads_port: entry.reads_port,
            multi_arch: entry.multi_arch,
        }
    }

    /// The user this row moves, when it moves one.
    pub(super) fn moved_user(&self) -> Option<(&str, &str)> {
        (self.new_user != self.old_user).then_some((&self.old_user, &self.new_user))
    }
}

/// What a manually added model's branch resolves to today: `(commit, tag)`,
/// or `None` when the lookup could not be made.
pub(super) type FollowFn<'a> = &'a dyn Fn(&str, &ManualModel) -> Option<(String, String)>;

/// Resolve every enabled model without changing anything: the marketplace
/// ones against the fresh registry, the manual ones against `follow`.
///
/// `follow` is what a manually added model's branch resolves to today -
/// `(commit, tag)` - or `None` when the lookup could not be made. A manual
/// entry is never an [`ChapError::UnknownModel`]: the deployment is where its
/// definition lives, so there is nothing for the catalogue to have dropped.
pub(super) fn plan(
    project: &Project,
    registry: &Registry,
    follow: FollowFn,
) -> Result<Vec<ModelUpdate>> {
    let mut out = Vec::with_capacity(project.state.models.len());
    for (id, entry) in &project.state.models {
        let mut update = ModelUpdate::unchanged(id, entry);
        if let Some(manual) = project.state.manual.get(id) {
            update.manual = true;
            update.follow = manual.follow.clone();
            // A manual entry follows a branch or it is pinned; the channel
            // `.chap-devops/models.yaml` records only says which of the two.
            update.pinned = manual.follow.is_none();
            if manual.follow.is_some() {
                match follow(id, manual) {
                    Some((commit, tag)) => {
                        update.changed = tag != update.old_tag;
                        update.new_commit = Some(commit);
                        // The version of a manual entry is its tag: there is
                        // no version number to carry.
                        update.new_version = tag.clone();
                        update.new_tag = tag;
                    }
                    None => update.checked = false,
                }
            }
            out.push(update);
            continue;
        }
        if let Some(channel) = entry.channel {
            let model = registry
                .get(id)
                .ok_or_else(|| ChapError::UnknownModel(id.clone()))?;
            let version = model.resolve(&VersionSelector::Channel(channel))?;
            update.new_version = version.version.clone();
            update.new_tag = version.image_tag.clone();
            update.new_digest = version.image_digest.clone();
            // A new digest under the same tag is a change as well: the
            // marketplace now pins another image than the one that runs.
            update.changed = update.new_version != update.old_version
                || update.new_tag != update.old_tag
                || update.new_digest != update.old_digest;
        }
        out.push(update);
    }
    Ok(out)
}

/// Re-read the user of every marketplace model the plan moves to a new tag.
///
/// A manual entry is left alone: `models add` resolved its user against the
/// image, and `models remove` plus a fresh add is how that answer is revisited.
/// A row that is not moving is left alone too - the image behind it has not
/// changed, so neither has what it runs as.
pub(super) fn resolve_users(
    project: &Project,
    models: &mut [ModelUpdate],
    endpoints: &manual::Endpoints,
    warnings: &mut Vec<String>,
) {
    for update in models.iter_mut().filter(|m| m.changed && !m.manual) {
        let Some(entry) = project.state.models.get(&update.id) else {
            continue;
        };
        let resolution = resolve::from_image(
            &resolve::Request {
                id: &update.id,
                image: &entry.image,
                image_tag: &update.new_tag,
                image_digest: update.new_digest.as_deref(),
                data_dir_flag: None,
                user_flag: None,
            },
            endpoints,
        );
        warnings.extend(resolution.notes.iter().cloned());
        update.new_user = resolution.user;
        update.user_from = resolution.user_from;
        update.reads_port = resolution.reads_port;
        update.multi_arch = resolution.arm64;
    }
}

/// The newest published build of a manually added model's branch, as
/// `chap-devops update` asks for it.
///
/// Every failure is a warning and a `None`: a repository that will not answer
/// must not stop the marketplace half of the update, and leaving the pin
/// where it is is the safe half of that bargain.
pub(super) fn newest_published(
    id: &str,
    entry: &ManualModel,
    endpoints: &manual::Endpoints,
    warnings: &mut Vec<String>,
) -> Option<(String, String)> {
    let (Some(repository), Some(branch)) = (&entry.repository, &entry.follow) else {
        warnings.push(format!(
            "{id} follows a branch but records no repository, so its pin cannot be checked"
        ));
        return None;
    };
    match manual::newest_published(repository, branch, endpoints) {
        Ok(Some(found)) => Some(found),
        Ok(None) => {
            warnings.push(format!(
                "{repository} has no published `sha-` build on {branch}, so {id} keeps the pin it has"
            ));
            None
        }
        Err(err) => {
            warnings.push(format!(
                "could not check {repository} for a newer build of {id} ({err:#}); \
                 the pin stays at {}",
                entry.tag
            ));
            None
        }
    }
}
