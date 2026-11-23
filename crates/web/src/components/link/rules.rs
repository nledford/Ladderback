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
    /// A route is a path of this site, written as the browser keeps it, so
    /// the router's location matches it and never leaves the site: it starts
    /// with one `/`, and its path holds no empty segment (`//host` is another
    /// site), no `.` or `..` segment, plain or percent-encoded (which the
    /// browser folds, `/.//host` into `//host`), and no backslash, plain or
    /// `%5C` (which the browser reads as a `/`, and the router decodes). It
    /// is printable ASCII without `"`, `<`, `>`, `\`, `^`, `` ` ``, `{`, `|`
    /// or `}`, and its query and fragment have no `'`: the browser would
    /// percent-encode each, so a caller encodes them.
    NotAPath,
    /// An outside address is an `http` or `https` address with a host, and
    /// holds no space, control character or backslash: `javascript:` and
    /// `data:` are never one.
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
        if of_this_site(href) {
            Ok(Self {
                href: href.to_owned(),
                matching,
            })
        } else {
            Err(NotADestination::NotAPath)
        }
    }

    /// The link's `href`, as it was parsed.
    pub fn href(&self) -> &str {
        &self.href
    }

    /// Whether the reader is on this route's page, from the router's
    /// location: its path and its query (`?` before it or not).
    pub fn is_current(&self, path: &str, query: &str) -> bool {
        let (own_path, own_query) = parts(&self.href);
        let (own_path, path) = (trimmed(own_path), trimmed(path));
        match self.matching {
            Matching::Exact => {
                own_path == path && own_query == query.strip_prefix('?').unwrap_or(query)
            }
            Matching::Section => path
                .strip_prefix(own_path)
                .is_some_and(|below| below.is_empty() || below.starts_with('/')),
        }
    }
}

/// Whether text holds no space and no control character.
fn plain(text: &str) -> bool {
    !text.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Whether `href` is a path of this site the browser keeps as it is written
/// ([`NotADestination::NotAPath`]).
fn of_this_site(href: &str) -> bool {
    let (path, rest) = href.split_at(href.find(['?', '#']).unwrap_or(href.len()));
    let Some(path) = path.strip_prefix('/') else {
        return false;
    };
    let kept = href
        .chars()
        .all(|c| c.is_ascii_graphic() && !"\"<>\\^`{|}".contains(c));
    let last = path.split('/').count() - 1;
    let segments_kept = path.split('/').enumerate().all(|(at, segment)| {
        let empty_inside = segment.is_empty() && at < last;
        let dots = segment.to_ascii_lowercase().replace("%2e", ".");
        !empty_inside && dots != "." && dots != ".." && !dots.contains("%5c")
    });
    kept && segments_kept && !rest.contains('\'')
}

/// A route's path and its query (empty without one), its fragment dropped.
fn parts(href: &str) -> (&str, &str) {
    let href = href.split_once('#').map_or(href, |(before, _)| before);
    href.split_once('?').unwrap_or((href, ""))
}

/// A path without the slashes it ends in: `/operations/` is `/operations`,
/// and `/` is empty, which every path starts with.
fn trimmed(path: &str) -> &str {
    path.trim_end_matches('/')
}

/// An address on another site.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outside(String);

impl Outside {
    /// Parses an `http` or `https` address with a host.
    pub fn parse(address: &str) -> Result<Self, NotADestination> {
        let rest = ["https://", "http://"].into_iter().find_map(|scheme| {
            address
                .get(..scheme.len())
                .filter(|start| start.eq_ignore_ascii_case(scheme))
                .and_then(|_| address.get(scheme.len()..))
        });
        let has_host = rest.is_some_and(|rest| {
            rest.split(['/', '?', '#'])
                .next()
                .map(|authority| {
                    authority
                        .rsplit_once('@')
                        .map_or(authority, |(_, host)| host)
                })
                .is_some_and(|host| !host.is_empty() && !host.starts_with(':'))
        });
        if has_host && plain(address) && !address.contains('\\') {
            Ok(Self(address.to_owned()))
        } else {
            Err(NotADestination::NotAWebAddress)
        }
    }

    /// The link's `href`, as it was parsed.
    pub fn href(&self) -> &str {
        &self.0
    }
}

impl Destination {
    /// A page of the app at `href`, as a page writes its own paths: `None`
    /// for a path [`Route::parse`] refuses, which the page then shows
    /// without a link rather than one that could leave the site.
    pub fn route(href: &str, matching: Matching) -> Option<Self> {
        Route::parse(href, matching).ok().map(Self::Route)
    }

    /// An address on another site: `None` for one [`Outside::parse`]
    /// refuses, as [`Destination::route`] is for a path.
    pub fn outside(address: &str) -> Option<Self> {
        Outside::parse(address).ok().map(Self::Outside)
    }

    /// The link's `rel`: an outside link is never routed, even an address
    /// that turns out to be the app's own (`external`, which the router
    /// leaves to the browser), and tells the site it opens nothing of the
    /// page it came from (`noreferrer`, which implies `noopener`); a route
    /// has none.
    pub fn rel(&self) -> Option<&'static str> {
        match self {
            Self::Route(_) => None,
            Self::Outside(_) => Some("external noreferrer"),
        }
    }

    /// The link's `aria-current`: `page` on a route the reader is on, from
    /// the router's location, so it can't drift from it; an outside link is
    /// never the current page.
    pub fn current(&self, path: &str, query: &str) -> Option<&'static str> {
        match self {
            Self::Route(route) => route.is_current(path, query).then_some("page"),
            Self::Outside(_) => None,
        }
    }

    /// Whether the link says it leaves the app: words for a screen reader
    /// after its own, and an icon after its own words too.
    pub fn leaves(&self) -> bool {
        matches!(self, Self::Outside(_))
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
        match self {
            Self::Primary => Tone::Primary,
            Self::Inherit => Tone::Inherit,
        }
    }
}

/// Where a link stands, which decides whether it is underlined at rest
/// (#1452). Every link names one: none is the default, so each page decides
/// for each of its links.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkPlacement {
    /// Among words: a sentence, a paragraph, a banner or an alert. Always
    /// underlined, so it is never told from the words around it by its
    /// color alone (1.4.1): Ladder Blue is under 3:1 against the text, and
    /// the text's own color is 1:1.
    InText,
    /// On its own: the header's links, a menu's rows, a link alone in a
    /// table's cell or in a list of links. No underline at rest: nothing
    /// around it is words it could be mistaken for.
    Alone,
}

/// How a link of this placement is drawn, in Tailwind utilities.
fn look(placement: LinkPlacement) -> &'static str {
    let _ = placement;
    todo!()
}

/// How a link is drawn, in Tailwind utilities (decision 117), logical ones
/// only: always underlined, so a link in a line of text is never told by its
/// color alone (1.4.1), and its color needs no 3:1 against the words around
/// it. The underline is a token's thickness (`--link-underline`), drawn far
/// enough under the baseline to clear the letters' descenders
/// (`--link-underline-offset`), which ink skipping keeps it off besides, and
/// thicker under the pointer (`--link-underline-hover`); focus keeps the
/// global ring. The page it names is bold (The Weight Not Color Rule). An
/// outside link's icon stands a quarter of an em after its words. Its color
/// is `style.css`'s, which says why.
const LOOK: &str = "underline decoration-(length:--link-underline) \
    underline-offset-(--link-underline-offset) [text-decoration-skip-ink:auto] \
    hover:decoration-(length:--link-underline-hover) aria-[current=page]:font-bold \
    [&>.icon]:ms-[0.25em]";

/// The classes of the link: the link's own, which names it and colors it in
/// `style.css`, its tone's, and its look.
pub fn link_class(tone: LinkTone) -> String {
    format!("link {} {LOOK}", tone.tone().class())
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
    fn a_page_links_to_a_path_or_an_address_only_once_it_parses() {
        assert_eq!(
            Destination::route("/plans", Matching::Section),
            Some(Destination::Route(route("/plans", Matching::Section)))
        );
        assert_eq!(Destination::route("//example.com", Matching::Exact), None);
        assert_eq!(
            Destination::outside("https://rustic.cli.rs/"),
            Some(Destination::Outside(Outside(
                "https://rustic.cli.rs/".to_owned()
            )))
        );
        assert_eq!(Destination::outside("javascript:alert(1)"), None);
    }

    #[test]
    fn a_link_is_drawn_in_ladder_blue_unless_its_page_names_the_text_around_it() {
        assert_eq!(LinkTone::default(), LinkTone::Primary);
        assert_eq!(LinkTone::Primary.tone(), Tone::Primary);
        assert_eq!(LinkTone::Inherit.tone(), Tone::Inherit);
        for (tone, class) in [
            (LinkTone::Primary, "tone-primary"),
            (LinkTone::Inherit, "tone-inherit"),
        ] {
            assert_eq!(link_class(tone), format!("link {class} {LOOK}"), "{tone:?}");
        }
    }

    fn classes(placement: LinkPlacement) -> Vec<&'static str> {
        look(placement).split_whitespace().collect()
    }

    #[test]
    fn a_link_in_text_is_underlined_at_rest_and_one_standing_alone_only_under_the_pointer_or_pressed()
     {
        let in_text = classes(LinkPlacement::InText);
        assert!(in_text.contains(&"underline"), "{in_text:?}");
        assert!(!in_text.contains(&"no-underline"), "{in_text:?}");

        let alone = classes(LinkPlacement::Alone);
        assert!(alone.contains(&"no-underline"), "{alone:?}");
        assert!(!alone.contains(&"underline"), "{alone:?}");
        assert!(alone.contains(&"hover:underline"), "{alone:?}");
        assert!(alone.contains(&"active:underline"), "{alone:?}");
    }

    #[test]
    fn the_current_page_is_bold_wherever_a_link_stands_and_barred_at_its_start_when_alone() {
        for placement in [LinkPlacement::InText, LinkPlacement::Alone] {
            assert!(
                classes(placement).contains(&"aria-[current=page]:font-bold"),
                "{placement:?}"
            );
        }
        assert!(classes(LinkPlacement::Alone).contains(&"aria-[current=page]:border-s-3"));
        assert!(!classes(LinkPlacement::InText).contains(&"aria-[current=page]:border-s-3"));
    }
}
