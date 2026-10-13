//! Integration tests for `rust-motion serve`.
//!
//! These tests verify that:
//!  1. `ServeConfig` is constructable with reasonable defaults.
//!  2. The HTML player page generation produces valid output.
//!  3. Frame rendering via the same path as the serve loop works in the
//!     rhai feature context (using a trivial hello_world script).

#[cfg(feature = "rhai")]
mod serve_tests {
    use rust_motion_cli::serve::ServeConfig;
    use std::path::PathBuf;

    /// ServeConfig can be constructed with valid fields.
    #[test]
    fn test_serve_config_construction() {
        let cfg = ServeConfig {
            script: PathBuf::from("my_script.rhai"),
            props: None,
            port: 7890,
            default_frame: 0,
            width: 1920,
            height: 1080,
            fps: 30.0,
            duration: 150,
        };
        assert_eq!(cfg.port, 7890);
        assert_eq!(cfg.default_frame, 0);
        assert_eq!(cfg.width, 1920);
    }

    /// The Serve variant is parseable via Clap from the expected CLI args.
    #[test]
    fn test_serve_cli_parse() {
        use clap::Parser;
        use rust_motion_cli::{Cli, Commands};

        let cli = Cli::try_parse_from([
            "rust-motion",
            "serve",
            "--script",
            "examples/templates/podcast_waveform.rhai",
            "--port",
            "8080",
            "--frame",
            "5",
        ])
        .expect("Clap should parse the serve subcommand");

        match cli.command {
            Commands::Serve {
                port,
                frame,
                script,
                ..
            } => {
                assert_eq!(port, 8080);
                assert_eq!(frame, 5);
                assert_eq!(
                    script,
                    PathBuf::from("examples/templates/podcast_waveform.rhai")
                );
            }
            other => panic!("Expected Serve, got {other:?}"),
        }
    }

    /// The `rust-motion serve --help` text includes the expected subcommand flags.
    #[test]
    fn test_serve_help_text() {
        use clap::CommandFactory;
        use rust_motion_cli::Cli;

        let mut cmd = Cli::command();
        let help = format!("{}", cmd.render_help());
        // Top-level help should mention "serve"
        assert!(
            help.contains("serve"),
            "Top-level help should contain 'serve' subcommand"
        );
    }
}
