# Git

- Work on a branch (`next`, `release/X.Y.Z`, a feature name) and merge into `main` locally: `git merge --no-ff <branch> -m "Merge <branch>: <summary>"`.
- Commit subjects: `feat:`, `fix:`, `docs:`, `ci:`, `chore:`, then a short summary of what changes for users. End with the Co-Authored-By line.
- Every push runs the tests; pushing a `v*` tag releases (see [releases.md](releases.md)). Neither happens without the owner's go.
- Pushes with GIFs or other large files can fail with HTTP 400: `git -c http.postBuffer=157286400 push`.
