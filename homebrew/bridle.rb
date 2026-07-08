class Bridle < Formula
  desc "Sync MCP servers, skills, and agents across all AI coding harnesses"
  homepage "https://github.com/KristianLentino99/bridle"
  url "https://github.com/KristianLentino99/bridle/archive/refs/tags/v0.2.0.tar.gz"
  sha256 "26848ff4c2f475d4684b85337b1dd175dc9d1d4cb651de8e973dcc7b58e2ce8a"
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
