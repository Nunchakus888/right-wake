#!/usr/bin/env python3
"""Repository-only structural checks; not a substitute for a real host install."""
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parent.parent
PLUGIN = ROOT / "plugins/codex"


def require(condition, message):
    if not condition:
        raise ValueError(message)


def check():
    manifest = json.loads((PLUGIN / "plugin.json").read_text())
    version = re.search(r'^version\s*=\s*"([^"]+)"', (ROOT / "Cargo.toml").read_text(), re.M).group(1)
    require(manifest["name"] == "waking", "plugin name differs from binary")
    require(manifest["version"] == version, "plugin version differs from crate")
    require(manifest["$schema"] == "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "portable plugin schema missing")
    require(manifest["repository"] == "https://github.com/Nunchakus888/waking", "repository URL differs")
    require(manifest["license"] == "Apache-2.0", "license differs")
    extension = manifest["extensions"]["com.openai"]
    onboarding = extension["onboardingSkill"]
    require(onboarding.startswith("./skills/") and ".." not in onboarding, "invalid onboarding path")
    require((PLUGIN / onboarding).is_file(), "onboarding skill is missing")
    require(extension["requires_local_executor"] is True, "plugin must declare local executor")

    mcp = json.loads((PLUGIN / "mcp.json").read_text())
    require(mcp["$schema"] == "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json", "portable MCP schema missing")
    require(set(mcp["mcpServers"]) == {"waking"}, "unexpected MCP server")
    server = mcp["mcpServers"]["waking"]
    require(server["type"] == "stdio" and server["command"] == "sh", "MCP transport changed")
    require(server["args"] == ["${PLUGIN_ROOT}/scripts/mcp.sh"], "MCP launcher must be plugin-contained")
    require((PLUGIN / "scripts/mcp.sh").is_file(), "MCP launcher missing")
    installer = (ROOT / "scripts/install.sh").read_text()
    require((PLUGIN / "scripts/install.sh").read_text() == installer, "cached plugin installer differs from the release installer")
    require(f"version=${{WAKING_VERSION:-v{version}}}" in installer, "native installer default differs from crate")
    require(f"scripts/install.sh --version v{version}" in (PLUGIN / onboarding).read_text(), "setup skill installer pin differs from crate")
    for document in (PLUGIN / "skills").rglob("*.md"):
        for link in re.findall(r"\]\(([^)]+)\)", document.read_text()):
            if re.match(r"^[a-zA-Z][a-zA-Z0-9+.-]*:", link) or link.startswith("#"):
                continue
            target = (document.parent / link.split("#", 1)[0]).resolve()
            require(target.is_relative_to(PLUGIN.resolve()), f"skill link escapes cached plugin: {document}: {link}")
            require(target.exists(), f"skill reference is missing: {document}: {link}")

    market = json.loads((ROOT / ".agents/plugins/marketplace.json").read_text())
    entries = market["plugins"]
    require(len(entries) == 1 and entries[0]["name"] == "waking", "marketplace entry differs")
    require(entries[0]["source"] == {"source": "local", "path": "./plugins/codex"}, "marketplace path must be repository-relative")
    require(entries[0]["policy"] == {"installation": "AVAILABLE", "authentication": "ON_INSTALL"}, "marketplace policy missing")
    require(entries[0]["category"] == "Productivity", "marketplace category missing")

    skills = sorted((PLUGIN / "skills").glob("*/SKILL.md"))
    require(len(skills) == 3, "expected setup, route, and review skills")
    for file in skills:
        text = file.read_text()
        require(text.startswith("---\n") and "\n---\n" in text[4:], f"missing frontmatter: {file}")
        frontmatter = text.split("---", 2)[1]
        name = re.search(r"^name: ([a-z0-9-]+)$", frontmatter, re.M)
        require(name is not None and name.group(1) == file.parent.name, f"skill name differs: {file}")
        require(re.search(r"^description: .+", frontmatter, re.M), f"description missing: {file}")
        require("TODO" not in text and "[INSERT" not in text, f"unfinished skill: {file}")
        metadata = (file.parent / "agents/openai.yaml").read_text()
        require("$" + file.parent.name in metadata, f"default prompt must name skill: {file}")
    print("Plugin paths, metadata, and three skills are consistent. Real Codex install remains a separate check.")


if __name__ == "__main__":
    try:
        check()
    except (KeyError, OSError, ValueError, AttributeError) as error:
        print(f"Plugin check failed: {error}", file=sys.stderr)
        sys.exit(1)
