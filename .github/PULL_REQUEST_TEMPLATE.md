## Summary

<!-- What does this change and why? Link the issue it addresses. -->

## Checklist

- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` passes
- [ ] `cargo test` passes
- [ ] `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features` passes
- [ ] `cargo deny check` passes
- [ ] Every source file is 250 lines or fewer
- [ ] No `unwrap()` or `expect()` added to library code
- [ ] Tests added or updated using `FakeDevice` (`tests/common/fake`)
- [ ] `CHANGELOG.md` updated under "Unreleased"
- [ ] PR title follows Conventional Commits (for example `feat(sync): ...`)
