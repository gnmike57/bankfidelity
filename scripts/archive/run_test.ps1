$env:Path = "$env:USERPROFILE\.cargo\bin;" + $env:Path
cargo test --test au_transfer_stress -- --ignored
