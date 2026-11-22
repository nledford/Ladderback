# Pull request assets

This branch holds the screenshots and other images that pull request descriptions show. It shares no history with `main` and is never merged.

Each pull request has its own folder, named after its branch or issue (`1347-hamburger-menu/`, say). A description links an image as:

```
https://raw.githubusercontent.com/nledford/Ladderback/pr-assets/<folder>/<file>.png
```

## Adding a folder

Add one new folder in a new commit on top of this branch, and change no existing folder: older descriptions still link to them. If the push is refused because someone else pushed first, fetch the branch and add the folder again on top; folders never conflict.

Screenshots show only neutral demo data, never a real library, password, host name or key.
