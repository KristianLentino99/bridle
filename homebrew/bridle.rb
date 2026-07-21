class Bridle < Formula
  desc "Sync MCP servers, skills, and agents across all AI coding harnesses"
  homepage "https://github.com/KristianLentino99/bridle"
  url "https://github.com/KristianLentino99/bridle/archive/refs/tags/v0.5.0.tar.gz"
  sha256 "b59254bc5b74a1ad8ed0083b1d8b42f053aba58093e1cd2e07601a6124585e22"
  license "MIT"
  head "https://github.com/KristianLentino99/bridle.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    assert_match "Sync MCP servers", shell_output("#{bin}/bridle --help")

    system bin/"bridle", "init"
    assert_path_exists testpath/"Bridle/mcp.json"
    assert_path_exists testpath/"Bridle/config.json"
  end
end
