//! Release signing tool (maintainers and the release workflow).
//!
//!     oxy-update-sign keygen              print a new secret key and its public key
//!     oxy-update-sign public              print the public key for $OXY_UPDATE_SIGNING_KEY
//!     oxy-update-sign sign FILE           write FILE.sig signed with $OXY_UPDATE_SIGNING_KEY
//!
//! The secret key is read from the environment, never from arguments, so
//! it doesn't end up in shell history or CI logs.

use oxy_update::verify;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let key = || std::env::var("OXY_UPDATE_SIGNING_KEY").map_err(|_| "set OXY_UPDATE_SIGNING_KEY".to_string());
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["keygen"] => verify::generate_key().map(|(secret, public)| {
            println!("secret key (store as the repository secret UPDATE_SIGNING_KEY; never commit it):\n{secret}");
            println!("public key (put in crates/oxy-update/src/lib.rs, SIGNING_PUBLIC_KEY):\n{public}");
        }),
        ["public"] => key().and_then(|k| verify::public_key(&k)).map(|p| println!("{p}")),
        ["sign", file] => key().and_then(|k| {
            let data = std::fs::read(file).map_err(|e| format!("{file}: {e}"))?;
            let sig = verify::sign(&data, &k)?;
            let out = format!("{file}.sig");
            std::fs::write(&out, format!("{sig}\n")).map_err(|e| format!("{out}: {e}"))?;
            println!("wrote {out}");
            Ok(())
        }),
        _ => Err("usage: oxy-update-sign keygen | public | sign FILE".into()),
    };
    if let Err(e) = result {
        eprintln!("oxy-update-sign: {e}");
        std::process::exit(1);
    }
}
