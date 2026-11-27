#!/usr/bin/env bash
# `just push [git push args]`: runs `just prepush` on HEAD and pushes HEAD to its branch on origin only when prepush exited 0 and
# HEAD and the tree are what was tested. No git hook is installed: other sessions' `git push` behaves as before.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

branch=$(git rev-parse --abbrev-ref HEAD)
case "$branch" in
  HEAD) echo "push: detached HEAD; check out a branch"; exit 2 ;;
  main) echo "push: main is protected: push a branch and open a pull request"; exit 2 ;;
esac
if [ -n "$(git status --porcelain --untracked-files=no)" ]; then
  echo "push: the tree has uncommitted changes; prepush would test them, not what is pushed. Commit first."
  exit 2
fi
sha=$(git rev-parse HEAD)
echo "push: testing $sha ($branch)"

set +e
bun tools/prepush/prepush.ts
rc=$?
set -e
if [ "$rc" -ne 0 ]; then
  echo "push: prepush exited $rc; nothing pushed"
  exit "$rc"
fi
now=$(git rev-parse HEAD)
if [ "$now" != "$sha" ] || [ -n "$(git status --porcelain --untracked-files=no)" ]; then
  echo "push: HEAD or the tree changed while prepush ran ($sha -> $now); nothing pushed. Run just push again."
  exit 1
fi
echo "push: prepush passed for $sha; pushing"
git push "$@" origin "$sha:refs/heads/$branch"
