@ui
Feature: The web UI's files arrive compressed
  The page's module is by far the largest file the web UI loads. A release
  build of the web UI keeps a Brotli and a gzip copy of each file that
  compression makes smaller, and the daemon sends the copy the browser says it
  can read, so a page opened over a slow link loads in fewer bytes. A browser
  that reads neither gets the file as it is (§10.1).

  Scenario Outline: A browser that reads <accepts> gets the page's module <how>
    When I open the web UI
    And the browser asks for the page's module again, saying it reads "<accepts>"
    Then the module arrives with the content coding "<coding>"
    And it is the module the page runs
    And the answer says a cache must keep one copy for each coding a browser reads

    Examples:
      | accepts        | how                    | coding   |
      | gzip, br       | compressed with Brotli | br       |
      | gzip           | compressed with gzip   | gzip     |
      | identity       | as it is               | identity |
