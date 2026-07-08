class Bridle < Formula
  desc "Sync MCP servers, skills, and agents across all AI coding harnesses"
  homepage "https://github.com/KristianLentino99/bridle"
  url "https://github.com/KristianLentino99/bridle/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "0d8f58a1bd67c25b5a6e562ae61f58b129af04dafd1cefdc5499cbcc009be6b1"
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
