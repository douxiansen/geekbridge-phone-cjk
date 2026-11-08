# Releasing

Installed apps update themselves from GitHub releases, so shipping a change is:

1. Bump `version` in the root `Cargo.toml` and add a `## <version>` section to `CHANGELOG.md`
   (its text becomes the release notes shown in the app).
2. Commit, then `git tag v<version> && git push origin main v<version>`.
3. The **Release** workflow tests, builds, packages and signs, and publishes the GitHub
   release with `geekbridge-phone-<version>-linux-x86_64.tar.gz`, its `.sig`, and `SHA256SUMS`.
4. Running apps notice the new release (within a day, or via *Settings > Updates > Check now*),
   download it, verify the checksum **and** the Ed25519 signature against the public key built
   into the app, replace themselves and restart.

## The signing key

- The private key is the repository secret `UPDATE_SIGNING_KEY` (hex). Keep an offline backup:
  if it is lost, every installed app must be reinstalled by hand, because they only trust the
  public key compiled into them (`crates/phone-update/src/public_key.hex`).
- To rotate: `cargo run -p phone-update --bin gbp-keygen` prints the private key on stdout and
  the public key on stderr. Put the private key in the secret and the public key in
  `public_key.hex`, and ship one release signed with the **old** key that contains the new public
  key. Releases signed with the new key are accepted only after that.

## Rolling back

The updater keeps the previous program next to the new one as `geekbridge-phone.previous`.
Roll back with `mv ~/.local/bin/geekbridge-phone.previous ~/.local/bin/geekbridge-phone` and
restart the service. Turn automatic checks off in *Settings > Updates* or with
`"check_updates": false` in `~/.config/geekbridge-phone/settings.json`.
