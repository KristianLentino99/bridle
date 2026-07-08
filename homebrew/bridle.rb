# Homebrew Formula for bridle
#
# Usage after publishing the GitHub release:
#   brew install kristianlentino/tap/bridle
#
# To create the tap:
#   1. Create GitHub repo: kristianlentino/homebrew-tap
#   2. Copy this file to: homebrew-tap/Formula/bridle.rb
#   3. Update SHA256 hashes from the GitHub release
class Bridle < Formula
  desc "Sync MCP servers, skills, and agents across all AI coding harnesses"
  homepage "https://github.com/kristianlentino/bridle"
  url "https://github.com/kristianlentino/bridle/releases/download/v0.1.0/bridle-v0.1.0-aarch64-apple-darwin.tar.gz"
  sha256 "REPLACE_WITH_ACTUAL_SHA256"
  license "MIT"
  version "0.1.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/kristianlentino/bridle/releases/download/v0.1.0/bridle-v0.1.0-aarch64-apple-darwin.tar.gz"
      sha256 "REPLACE_WITH_ACTUAL_SHA256_ARM"
    end
    if Hardware::CPU.intel?
      url "https://github.com/kristianlentino/bridle/releases/download/v0.1.0/bridle-v0.1.0-x86_64-apple-darwin.tar.gz"
      sha256 "REPLACE_WITH_ACTUAL_SHA256_INTEL"
    end
  end

  on_linux do
    if Hardware::CPU.intel?
      url "https://github.com/kristianlentino/bridle/releases/download/v0.1.0/bridle-v0.1.0-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "REPLACE_WITH_ACTUAL_SHA256_LINUX"
    end
  end

  def install
    bin.install "bridle-#{version}-aarch64-apple-darwin" => "bridle" if OS.mac? && Hardware::CPU.arm?
    bin.install "bridle-#{version}-x86_64-apple-darwin" => "bridle" if OS.mac? && Hardware::CPU.intel?
    bin.install "bridle-#{version}-x86_64-unknown-linux-gnu" => "bridle" if OS.linux?
  end

  test do
    system "#{bin}/bridle", "--version"
  end
end
