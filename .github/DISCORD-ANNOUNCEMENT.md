# Discord announcement: the issue list is open

Ready-to-paste posts for the community Discord (<https://discord.gg/ccQWu8rRcf>), inviting people
to pick up work from the issue list. Both fit Discord's 2000-character limit, so each one is a
single message.

- Pin the post in the channel where contributors arrive (`#contributing` or `#general`), and post
  it again when the issues it points at are taken — not on a schedule.
- Discord renders `**bold**`, `*italics*`, `` `code` `` and links; it has no headings or tables, so
  keep edits to those.
- The rules the post refers to are in [`CONTRIBUTING.md`](../CONTRIBUTING.md) › *Working on an
  issue*. Keep the two in step: if the claiming rules change, change this text in the same pull
  request.
- Before posting, check that the issues named or linked are still in `Backlog` or `Ready` and have
  no open pull request: <https://github.com/suhina-daw/suhina-daw/pulls?q=is%3Apr+is%3Aopen>.
- The `/claim` shortcut in both posts needs the `PROJECT_TOKEN` secret
  ([`claim-bot.md`](../docs/claim-bot.md)). Without it the bot answers in the issue instead of
  moving the card, and the comment alone is the claim — so the post is true either way.
- Both posts name `Do not start yet`, and `/hold` writes it: add that option to the board's `Status`
  field first, or the bot will say the field does not have it.

## English

```text
**🎛️ Looking for something to work on? The issue list is open**

Most of the open issues are free to pick up: features, bugs, docs, tests. Some are small, some are big.

• Good first issues → https://github.com/suhina-daw/suhina-daw/issues?q=is%3Aissue+is%3Aopen+label%3A%22good+first+issue%22
• All open issues → https://github.com/suhina-daw/suhina-daw/issues
• Board: what is already taken → https://github.com/orgs/suhina-daw/projects/1

**Before you start — one minute of checking**
1. **Is a pull request already open for it?** Look at the issue's *Linked pull requests* box, or the open pull requests: https://github.com/suhina-daw/suhina-daw/pulls?q=is%3Apr+is%3Aopen — one that says `Closes #123` has taken it.
2. **On the board it must be in Backlog or Ready.** *In progress* means someone is on it, and *Do not start yet* means exactly that — the issue says what the work waits for.
3. **Claim it in a comment:** *"I'd like to take this. Plan: … When: …"* — and set its Status to *In progress* if you can. The first comment wins; if someone was faster, take the next issue, there are plenty.
4. In your pull request write `Closes #123`, so the issue closes itself.

Shortcut: start your comment with `/claim` and the board moves itself and assigns you — and `/unclaim` puts it back if you stop.

Changed your mind, or stuck? Comment on the issue and set it back to *Ready*. That is normal, and far better than silence.

**First pull request ever?** Read CONTRIBUTING.md first, especially the clean-room rules and the `git commit -s` sign-off CI checks on pull requests from forks. Rust only, GPL-3.0-or-later. Build trouble? Ask here.

**Working with an AI agent?** It sees the issue and the repository, nothing else — put the plan in the issue, the result in the pull request, and you answer for what it wrote.
```

## Finnish

```text
**🎛️ Tekemistä etsimässä? Issuelista on auki**

Suurin osa avoimista issueista on vapaasti otettavissa: ominaisuuksia, bugeja, dokumentaatiota, testejä. Osa on pieniä, osa isoja.

• Hyvät aloitusissuet → https://github.com/suhina-daw/suhina-daw/issues?q=is%3Aissue+is%3Aopen+label%3A%22good+first+issue%22
• Kaikki avoimet issuet → https://github.com/suhina-daw/suhina-daw/issues
• Board: mikä on jo varattu → https://github.com/orgs/suhina-daw/projects/1

**Ennen kuin aloitat — minuutti tarkistusta**
1. **Onko siitä jo avoin pull request?** Katso issuen *Linked pull requests* -laatikko tai avoimet PR:t: https://github.com/suhina-daw/suhina-daw/pulls?q=is%3Apr+is%3Aopen — jos PR:ssä lukee `Closes #123`, se on varattu.
2. **Boardilla sen pitää olla Backlog tai Ready.** *In progress* tarkoittaa, että joku on jo aloittanut, ja *Do not start yet* tarkoittaa täsmälleen sitä — issue kertoo mitä työ odottaa.
3. **Ilmoittaudu kommentilla:** *"Otan tämän. Suunnitelma: … Aikataulu: …"* — ja laita Status *In progress*, jos voit. Ensimmäinen kommentti voittaa; jos joku ehti ensin, ota seuraava issue, tekemistä riittää.
4. Laita pull requestiin `Closes #123`, niin issue sulkeutuu itsestään.

Oikopolku: aloita kommentti `/claim`-sanalla, niin board siirtyy ja sinut assignataan — `/unclaim` palauttaa kortin, jos lopetat.

Mieli muuttui tai jäit jumiin? Kommentoi issueen ja laita Status takaisin *Ready*. Se on normaalia ja paljon parempi kuin hiljaisuus.

**Ensimmäinen pull request koskaan?** Lue CONTRIBUTING.md ensin — etenkin clean-room-säännöt ja `git commit -s` -allekirjoitus, jonka CI tarkistaa forkeista tulevilta PR:iltä. Vain Rustia, GPL-3.0-or-later. Buildin kanssa jumissa? Kysy täällä.

**Teetkö työn tekoälyagentilla?** Se näkee issuen ja repon, ei muuta — suunnitelma issueen, tulos pull requestiin, ja sinä vastaat siitä mitä se kirjoitti.
```
