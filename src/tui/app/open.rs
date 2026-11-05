//! What `o` and the image reference hand to the world outside the terminal.

use super::{App, Effect, OPEN_NEEDS_COMPONENT, OPEN_NEEDS_SAVING, Page};
use crate::open::Openable;

impl App<'_> {
    /// Hand what the row under the cursor points at to a browser: the page
    /// decides whether that is a repository or a web interface.
    pub(super) fn open_selection(&mut self) {
        match self.page {
            Page::Models => self.open_repository(),
            Page::Components => self.open_component(),
        }
    }

    /// Ask the caller to open the selected model's repository.
    pub(super) fn open_repository(&mut self) {
        let Some(row) = self.selected() else {
            return;
        };
        let url = self.model(row).source.repository.clone();
        self.effect = Some(Effect::Open(url));
    }

    /// Ask the caller to open the selected component's web interface, or say in
    /// the footer why there is none to open.
    ///
    /// Resolved against the *recorded* set, not the session's: what a browser
    /// can reach is what this deployment publishes now, and a row toggled or
    /// re-ported in this session is a plan until `s` has written it and
    /// `chap-devops up` has applied it. So a pending change is said rather than
    /// opened, and a pending port change opens the port that is actually
    /// published - which is the one the footer then names.
    pub(super) fn open_component(&mut self) {
        let component = self.selected_component();
        let recorded = crate::open::resolve(component, &self.initial_components, &self.api_base());
        // A component this session turned on has no compose file yet, so its
        // address belongs to nothing: that is a different answer from a
        // component the deployment simply does not have.
        if recorded == Openable::Off && self.components.is_enabled(component) {
            self.message = Some(OPEN_NEEDS_SAVING.to_string());
            return;
        }
        match recorded {
            Openable::Url { url, .. } => self.effect = Some(Effect::Open(url)),
            Openable::Internal { inside } => {
                self.message = Some(format!(
                    "{} publishes no host port; it is reached at {inside} inside the deployment, \
                     and p publishes one",
                    component.name()
                ))
            }
            Openable::NoWeb => self.message = Some(crate::open::NO_WEB_INTERFACE.to_string()),
            Openable::Off => self.message = Some(OPEN_NEEDS_COMPONENT.to_string()),
        }
    }

    /// Where chap-core's API is reached from this machine, as this browser knows
    /// it.
    ///
    /// The recorded API port, which is exactly what the REACH column prints:
    /// the browser reads no `.env`, so a `CHAP_API_PORT` or `CHAP_ROOT_PATH`
    /// written there is `chap-devops open`'s to honour and not this page's.
    fn api_base(&self) -> String {
        format!("http://localhost:{}", self.api_port)
    }

    /// Put the selected model's image reference on the status line, where it
    /// can be read off and copied by hand.
    pub(super) fn show_image_ref(&mut self) {
        let Some(row) = self.selected() else {
            return;
        };
        let model = self.model(row);
        let reference = match self.resolved(row) {
            Some(version) => model.image_ref(version),
            None => model.source.image.clone(),
        };
        self.message = Some(reference);
    }
}
