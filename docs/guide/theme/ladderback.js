// Accessibility fixes to mdBook's page that CSS can't make (WCAG 2.2 AA; see
// theme/ladderback.css for the rest). mdBook loads this after its own script.
(() => {
  // 2.4.1: a skip link, the first thing a Tab reaches, past the sidebar and
  // the menu bar to the page's text.
  const main = document.querySelector("main");
  if (main !== null) {
    main.id = "ladderback-main";
    main.setAttribute("tabindex", "-1");
    const skip = document.createElement("a");
    skip.className = "ladderback-skip-link";
    skip.href = "#ladderback-main";
    skip.textContent = "Skip to main content";
    document.body.prepend(skip);
  }

  // 4.1.2 and 2.1.1: mdBook's sidebar toggle is a <label> for a hidden
  // checkbox, so a keyboard can't reach it and its aria-label and
  // aria-expanded aren't allowed on it. As a button it is both.
  const toggle = document.getElementById("mdbook-sidebar-toggle");
  if (toggle !== null) {
    toggle.setAttribute("role", "button");
    toggle.setAttribute("tabindex", "0");
    toggle.addEventListener("keydown", (event) => {
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        toggle.click();
      }
    });
  }
})();
