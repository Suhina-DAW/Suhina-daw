# Rights and clean-room reports

Suhina reimplements the behaviour of commercial software without using its code, content or
visual design ([`CONTRIBUTING.md`](CONTRIBUTING.md) › Clean-room). If you believe something in
this repository breaks that rule, or infringes your copyright, trademark or design rights,
please tell us. These reports are handled before any other work.

## How to report

- **Privately (preferred):** use GitHub's private reporting for this repository (*Security* →
  *Report a vulnerability*) and start the title with `IP:`. It reaches the maintainers only.
- **Publicly:** open an issue titled `IP: …` describing *where* the problem is (file, commit,
  pull request) and *why*. Do not attach the protected material itself.

Please include who you are or whom you represent, the work you believe is infringed, and the
files or commits concerned.

## What happens

1. The maintainers acknowledge the report, normally within seven days.
2. While it is being looked at, the material in question is removed from the default branch or
   disabled, if the report is plausible on its face.
3. If the report is upheld, the material is removed for good, and so is anything derived from
   it (code written from a contaminated spec is rewritten from a clean one). When the material
   must not stay available at all, it is also removed from the git history: the history is
   rewritten with `git filter-repo`, the affected branches and tags are force-pushed, open pull
   requests are rebased, and GitHub Support is asked to purge cached views and pull request
   refs. Forks are outside our control; their owners are notified.
4. The contributor responsible is told why, and the outcome is recorded in the issue (without
   the material).

A contributor who learns that their own contribution used a forbidden source should report it
the same way; doing so early is always better than leaving it in.
