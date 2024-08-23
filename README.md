<div align="center">
    <h1>dontpanic</h1>
    <p>
        Send Rust panic!() and log::error!() messages to a backend server.
        See <a href="https://github.com/peterprototypes/dontpanic-server">dontpanic-server</a>
    </p>
</div>

[![crates.io](https://img.shields.io/crates/v/dontpanic?label=latest)](https://crates.io/crates/dontpanic)
[![Docs](https://docs.rs/dontpanic/badge.svg)](https://docs.rs/dontpanic)
![License](https://img.shields.io/crates/l/dontpanic.svg)

Client library for [Don't Panic Server](https://github.com/peterprototypes/dontpanic-server).
This crate registers a panic handler and send each panic from your application to a backend server.
If configured, the latest log messages before each panic are sent as well.
Supported logging facilities are log and tracing.

## Example Usage

To use `dontpanic`, add this to your `Cargo.toml`:

```toml
[dependencies]
dontpanic = "*"
```

```rust
use anyhow::Result;

fn main() -> Result<()> {
    let client = dontpanic::builder("<PROJECT_API_KEY>")
        .environment("production")
        .version(env!("CARGO_PKG_VERSION"))
        .build()?

    let logger = env_logger::Builder::from_default_env().build();
    client.set_logger(logger)?;

    log::info!("What's happening here?");
    log::error!("Booooom");

    Option::<u32>::None.unwrap();

    Ok(())
}
```

`<PROJECT_API_KEY>` can be obtained from [Don't Panic Server](https://github.com/peterprototypes/dontpanic-server). For more examples see the [Documentation](https://docs.rs/dontpanic).

## License

Licensed under either of

-   Apache License, Version 2.0
    ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
-   MIT license
    ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.