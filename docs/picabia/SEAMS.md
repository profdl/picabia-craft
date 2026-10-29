# Picabia seams in upstream FigCraft files

Every edit Picabia makes to a file that comes from upstream FigCraft is a *seam*: as few lines as
possible, calling into Picabia code, marked `// PICABIA: <reason>` (`rg "PICABIA:"` lists them).
When an upstream merge conflicts, resolve it seam by seam with this table, then run `cargo xtask ci`.

| File | Function / item | Why | How to re-apply |
|---|---|---|---|
| `.github/workflows/release.yml`, `sonarcloud.yml`, `freebsd.yml`, `windows-arm64.yml` | top job | Need storytold signing secrets, SonarCloud token or its runners/schedules | Re-add `if: github.repository == 'storytold/figcraft'` under the job name (4 lines in all). `ci.yml`, `nix.yml`, `packaging-lint.yml` call public reusable workflows and run unchanged on GitHub-hosted runners. |
