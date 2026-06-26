use std::fs;

fn main() {
    println!("cargo:rerun-if-changed=Cargo.toml");

    let manifest = fs::read_to_string("Cargo.toml").expect("read Cargo.toml");
    let tag = codex_git_tag(&manifest, "codex-login").expect("codex-login must have a tag");
    for crate_name in ["codex-model-provider", "codex-model-provider-info"] {
        let other = codex_git_tag(&manifest, crate_name).expect("codex dependency must have a tag");
        assert_eq!(
            tag, other,
            "all codex git dependencies must use the same tag"
        );
    }
    let version = tag.strip_prefix("rust-v").unwrap_or(tag);
    let version = version.strip_prefix('v').unwrap_or(version);

    println!("cargo:rustc-env=CODEX_AUTH_PROXY_CODEX_CLIENT_VERSION={version}");
}

fn codex_git_tag<'a>(manifest: &'a str, crate_name: &str) -> Option<&'a str> {
    let prefix = format!("{crate_name} = ");
    manifest
        .lines()
        .find(|line| line.trim_start().starts_with(&prefix))
        .and_then(|line| line.split("tag = ").nth(1))
        .and_then(|tag| tag.split('"').nth(1))
}
