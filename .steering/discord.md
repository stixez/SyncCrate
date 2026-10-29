# Discord

Server "SyncCrate" (guild 1564992487209046180), invite https://discord.gg/NY2P9u6v7Z (`DISCORD_INVITE` in the app).

## Release posts

- The `discord` job at the end of `release.yml` posts to #announcements through the `DISCORD_RELEASE_WEBHOOK` secret and pings the opt-in **Updates** role (1565000145802895427). It's part of release.yml, not its own workflow, because releases made with `GITHUB_TOKEN` don't trigger other workflows.
- GitHub activity (pushes, issues, PRs, stars) posts to #dev-log through a repo webhook.

## Roles

- Member roles are 6 genres (Life sim, Sandbox, Survival & co-op, RPG, Strategy & sim, MMO) plus Other games, picked in onboarding's "What do you play?". A new game needs no role: add a tag in #share-your-setup (max 20 tags, all used, so replace one) and, for a well-known title, a mention in its genre's onboarding description.
- **◆ SUPPORTER** (1565297770380656660) is for Buy Me a Coffee supporters, added by hand: donors put their Discord username in the coffee message. It unlocks nothing in the app.
- Staff and badge roles: ◆ ADMIN, ◆ MODERATOR, ◆ HELPER, ◆ CONTRIBUTOR, ◆ CREATOR (added by hand).

