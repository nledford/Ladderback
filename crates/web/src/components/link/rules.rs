//! Where a link goes and what it tells (components §4.5): a destination
//! parsed once into a route inside the app or an address on another site,
//! the attributes each gives its `<a>`, whether a route is the page the
//! reader is on, and the tone it is drawn in, taken from the shared
//! vocabulary (`variants.rs`). `components/link.rs`'s `Link` is the glue
//! that draws it.

use crate::variants::Tone;

/// Where a link goes. A link takes one of these, never a raw string it would
/// have to guess about: whether it stays in the app or leaves it decides how
/// it is drawn and what it says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Destination {
    /// A page of the app: a press is a route change, which titles the page
    /// and moves focus to its heading, never a reload.
    Route(Route),
    /// An address on another site: the link says it leaves, in words and
    /// with an icon, and tells that site nothing of the page it came from.
    Outside(Outside),
}

/// Why text isn't a destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotADestination {
    /// A route is a path of this site: it starts with one `/`, never two
    /// (`//host` is another site), never a backslash after it (which
    /// browsers read as a second `/`), and holds no space or control
    /// character.
    NotAPath,
    /// An outside address is an `http` or `https` address with a host, and
    /// holds no space or control character: `javascript:` and `data:` are
    /// never one.
    NotAWebAddress,
}

/// How a route link tells it is the page the reader is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Matching {
    /// The location is the link's path and its query: a page of a list
    /// (`/operations?page=2`) is the current page only on that page. A link
    /// without a query is current on its path only without one.
    Exact,
    /// The location is the link's path or a page below it, at a `/`: the
    /// navigation's `/snapshots` is current on `/snapshots/demo/3f2a`, never
    /// on `/snapshots-old`, whatever the query. A section at `/` holds every
    /// page.
    Section,
}

/// A page of the app: a path of this site, with an optional query and
/// fragment, and how it tells it is the current page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    href: String,
    matching: Matching,
}

impl Route {
    /// Parses a path of this site.
    pub fn parse(href: &str, matching: Matching) -> Result<Self, NotADestination> {
        let _ = (href, matching);
        todo!()
    }

    /// The link's `href`, as it was parsed.
    pub fn href(&self) -> &str {
        todo!()
    }

    /// Whether the reader is on this route's page, from the router's
    /// location: its path and its query (`?` before it or not).
    pub fn is_current(&self, path: &str, query: &str) -> bool {
        let _ = (path, query);
        todo!()
    }
}

/// An address on another site.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outside(String);

impl Outside {
    /// Parses an `http` or `https` address with a host.
    pub fn parse(address: &str) -> Result<Self, NotADestination> {
        let _ = address;
        todo!()
    }

    /// The link's `href`, as it was parsed.
    pub fn href(&self) -> &str {
        todo!()
    }
}

impl Destination {
    /// The link's `rel`: an outside link tells the site it opens nothing of
    /// the page it came from (`noreferrer`, which implies `noopener`); a
    /// route has none.
    pub fn rel(&self) -> Option<&'static str> {
        todo!()
    }

    /// The link's `aria-current`: `page` on a route the reader is on, from
    /// the router's location, so it can't drift from it; an outside link is
    /// never the current page.
    pub fn current(&self, path: &str, query: &str) -> Option<&'static str> {
        let _ = (path, query);
        todo!()
    }

    /// Whether the link says it leaves the app: words for a screen reader
    /// after its own, and an icon after them.
    pub fn leaves(&self) -> bool {
        todo!()
    }
}

/// The tones a link is drawn in. A link is not a status, so it has no
/// Secondary, Success, Warning or Error: each would leave its color the only
/// thing that said it is a link, or say a status a link doesn't have.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LinkTone {
    /// Ladder Blue (`--accent`), the one accent for action. The default.
    #[default]
    Primary,
    /// The color of the text around it, for a link inside a status
    /// sentence; still underlined, which says it is a link.
    Inherit,
}

impl LinkTone {
    /// The shared vocabulary's tone it is.
    pub fn tone(self) -> Tone {
        todo!()
    }
}

/// The classes of the link: the link's own, which underlines it, and its
/// tone's.
pub fn link_class(tone: LinkTone) -> String {
    let _ = tone;
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(href: &str, matching: Matching) -> Route {
        Route::parse(href, matching).unwrap_or_else(|refused| panic!("{href}: {refused:?}"))
    }

    #[test]
    fn a_route_is_a_path_of_this_site_kept_as_it_was_written() {
        for href in [
            "/",
            "/operations",
            "/snapshots/demo/3f2a",
            "/operations?page=2",
            "/log#end",
            "/snapshots/%C3%A9",
            "/%2F/example.com",
            "/plans/demo's",
            "/v1%2e2",
        ] {
            assert_eq!(route(href, Matching::Exact).href(), href);
        }
    }

    #[test]
    fn a_route_is_never_another_site_nor_a_path_with_spaces_or_control_characters() {
        for href in [
            "",
            "operations",
            "//example.com",
            "/\\example.com",
            "https://example.com/",
            "/demo plan",
            "/demo\tplan",
            "/demo\nplan",
            "/\u{7f}",
            "javascript:alert(1)",
            "/%5Cexample.com",
            "/%5cexample.com",
            "/demo\\plan",
            "/.//example.com",
            "/%2e//example.com",
            "/a/..//example.com",
            "/.\\/example.com",
            "/demo//plan",
            "/./demo",
            "/demo/..",
            "/%2E%2e/demo",
            "/.%2e/demo",
            "/snapshots/é",
            "/a{b}",
            "/a\"b",
            "/a<b>",
            "/a`b",
            "/a^b",
            "/a|b",
            "/plans?q='x'",
        ] {
            assert_eq!(
                Route::parse(href, Matching::Section),
                Err(NotADestination::NotAPath),
                "{href:?}"
            );
        }
    }

    #[test]
    fn an_outside_address_is_a_web_address_with_a_host_kept_as_it_was_written() {
        for address in [
            "https://rustic.cli.rs/",
            "http://hub.example:8080/docs?x=1",
            "HTTPS://Example.com",
            "https://example.com",
        ] {
            let outside = Outside::parse(address).unwrap_or_else(|refused| panic!("{refused:?}"));
            assert_eq!(outside.href(), address);
        }
    }

    #[test]
    fn an_outside_address_is_never_a_script_data_a_path_or_an_address_without_a_host() {
        for address in [
            "",
            "javascript:alert(1)",
            "data:text/html,hi",
            "/operations",
            "//example.com",
            "ftp://example.com/",
            "https://",
            "https:///path",
            "https://?q",
            "https://#top",
            "http:/example.com",
            "https://exa mple.com/",
            "https://example.com/\n",
            "https\u{0}://example.com",
            "https://@",
            "https://user@/path",
            "https://:443",
            "https://\\",
            "https://\\\\example.com",
            "https://example.com\\path",
        ] {
            assert_eq!(
                Outside::parse(address),
                Err(NotADestination::NotAWebAddress),
                "{address:?}"
            );
        }
    }

    #[test]
    fn an_exact_route_is_current_only_on_its_path_and_its_query() {
        let page = route("/operations?page=2", Matching::Exact);
        assert!(page.is_current("/operations", "?page=2"));
        assert!(
            page.is_current("/operations", "page=2"),
            "the router may drop the ?"
        );
        assert!(
            page.is_current("/operations/", "page=2"),
            "a trailing slash"
        );
        assert!(!page.is_current("/operations", "page=3"));
        assert!(!page.is_current("/operations", ""));
        assert!(!page.is_current("/operations/old", "page=2"));
        assert!(!page.is_current("/plans", "page=2"));

        let list = route("/operations", Matching::Exact);
        assert!(list.is_current("/operations", ""));
        assert!(
            list.is_current("/operations", "?"),
            "an empty query is none"
        );
        assert!(list.is_current("/operations/", ""));
        assert!(!list.is_current("/operations", "page=2"));
        assert!(!list.is_current("/operations/3f2a", ""));
        assert!(!list.is_current("/operation", ""));

        let slashed = route("/operations/", Matching::Exact);
        assert!(
            slashed.is_current("/operations", ""),
            "a trailing slash on the link"
        );

        let empty_query = route("/operations?", Matching::Exact);
        assert!(
            empty_query.is_current("/operations", ""),
            "an empty query on the link"
        );

        let start = route("/", Matching::Exact);
        assert!(start.is_current("/", ""));
        assert!(!start.is_current("/operations", ""));
    }

    #[test]
    fn an_exact_route_ignores_its_fragment() {
        let page = route("/log?level=warn#end", Matching::Exact);
        assert!(page.is_current("/log", "level=warn"));
    }

    #[test]
    fn a_section_route_is_current_on_its_path_and_below_it_at_a_slash_whatever_the_query() {
        let section = route("/snapshots", Matching::Section);
        assert!(section.is_current("/snapshots", ""));
        assert!(section.is_current("/snapshots/", ""));
        assert!(section.is_current("/snapshots/demo/3f2a", ""));
        assert!(section.is_current("/snapshots", "?repository=demo"));
        assert!(!section.is_current("/snapshots-old", ""));
        assert!(!section.is_current("/snapshot", ""));
        assert!(!section.is_current("/", ""));
        assert!(!section.is_current("/plans/snapshots", ""));

        let slashed = route("/snapshots/", Matching::Section);
        assert!(slashed.is_current("/snapshots", ""));
        assert!(slashed.is_current("/snapshots/demo", ""));
        assert!(!slashed.is_current("/snapshots-old", ""));

        let queried = route("/snapshots?repository=demo", Matching::Section);
        assert!(
            queried.is_current("/snapshots/demo/3f2a", ""),
            "a section's query is not its place"
        );

        let everything = route("/", Matching::Section);
        assert!(everything.is_current("/", ""));
        assert!(everything.is_current("/operations", ""));
    }

    #[test]
    fn only_a_route_the_reader_is_on_is_the_current_page() {
        let here = Destination::Route(route("/_gallery", Matching::Exact));
        assert_eq!(here.current("/_gallery", ""), Some("page"));
        assert_eq!(here.current("/", ""), None);

        let outside = Destination::Outside(Outside("https://rustic.cli.rs/".to_owned()));
        assert_eq!(outside.current("/_gallery", ""), None);
        assert_eq!(outside.current("https://rustic.cli.rs/", ""), None);
    }

    #[test]
    fn an_outside_link_leaves_is_marked_external_and_tells_its_site_nothing_and_a_route_does_neither()
     {
        let route = Destination::Route(route("/", Matching::Exact));
        assert_eq!(route.rel(), None);
        assert!(!route.leaves());

        let outside = Destination::Outside(Outside("https://rustic.cli.rs/".to_owned()));
        assert_eq!(outside.rel(), Some("external noreferrer"));
        assert!(outside.leaves());
    }

    #[test]
    fn a_link_is_drawn_in_ladder_blue_unless_its_page_names_the_text_around_it() {
        assert_eq!(LinkTone::default(), LinkTone::Primary);
        assert_eq!(LinkTone::Primary.tone(), Tone::Primary);
        assert_eq!(LinkTone::Inherit.tone(), Tone::Inherit);
        assert_eq!(link_class(LinkTone::Primary), "link tone-primary");
        assert_eq!(link_class(LinkTone::Inherit), "link tone-inherit");
    }
}
