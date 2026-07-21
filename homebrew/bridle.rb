class Bridle < Formula
  desc "Sync MCP servers, skills, and agents across all AI coding harnesses"
  homepage "https://github.com/KristianLentino99/bridle"
  url "https://github.com/KristianLentino99/bridle/archive/refs/tags/v0.4.0.tar.gz"
  sha256 "1d23dbc8d64aba37753d053245b0e1a8d400c7a3bc505508b380417368141241"
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
