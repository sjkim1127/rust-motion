# Homebrew Formula for Rust Motion CLI
# To use via tap:
#   brew tap sjkim1127/tap
#   brew install rust-motion

class RustMotion < Formula
  desc "Deterministic, browser-free programmatic video rendering platform"
  homepage "https://github.com/sjkim1127/rust-motion"
  version "1.0.0"
  license any_of: ["MIT", "Apache-2.0"]

  depends_on "ffmpeg"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/sjkim1127/rust-motion/releases/download/v#{version}/rust-motion-cli-macos-aarch64.tar.gz"
      # sha256 "<SHA256_AARCH64_MACOS>"
    else
      url "https://github.com/sjkim1127/rust-motion/releases/download/v#{version}/rust-motion-cli-macos-x86_64.tar.gz"
      # sha256 "<SHA256_X86_64_MACOS>"
    end
  end

  on_linux do
    if Hardware::CPU.intel?
      url "https://github.com/sjkim1127/rust-motion/releases/download/v#{version}/rust-motion-cli-linux-x86_64.tar.gz"
      # sha256 "<SHA256_X86_64_LINUX>"
    end
  end

  def install
    bin.install "rust-motion"
  end

  test do
    assert_match "rust-motion #{version}", shell_output("#{bin}/rust-motion --version")
    assert_match "diagnostics", shell_output("#{bin}/rust-motion doctor --json")
  end
end
