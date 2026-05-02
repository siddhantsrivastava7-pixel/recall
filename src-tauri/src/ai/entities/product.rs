//! Product / technology detector.
//!
//! Curated dictionary of well-known products and technologies.
//! Like `company`, hand-maintained — we'd rather miss obscure
//! libraries than false-match common nouns. The list overlaps
//! `company` for some entries (Google → company; Chrome →
//! product); deduplication at the top-level `detect_entities`
//! merges any cross-detector duplicates.
//!
//! No version-pattern detection in this v0.5.6 cut — `Tauri 2.0`
//! gets caught as `Tauri` (product), not `Tauri 2.0` (version).
//! Adding versioned variants is a v0.5.7 nice-to-have.

use super::{Entity, EntityType};

pub fn detect(content: &str) -> Vec<Entity> {
    let mut hits: Vec<Entity> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let lower = content.to_lowercase();
    for known in WHITELIST {
        let known_lower = known.to_lowercase();
        if find_word(&lower, &known_lower).is_some()
            && seen.insert(known_lower.clone())
        {
            hits.push(Entity {
                entity_type: EntityType::Product,
                entity_value: (*known).to_string(),
                raw_match: (*known).to_string(),
                confidence: 0.7,
            });
        }
    }
    hits
}

fn find_word(haystack: &str, needle: &str) -> Option<usize> {
    let mut start = 0;
    while let Some(idx) = haystack[start..].find(needle) {
        let absolute = start + idx;
        let before_ok = absolute == 0
            || !haystack
                .as_bytes()
                .get(absolute - 1)
                .map(|b| b.is_ascii_alphanumeric())
                .unwrap_or(false);
        let after_ok = haystack
            .as_bytes()
            .get(absolute + needle.len())
            .map(|b| !b.is_ascii_alphanumeric())
            .unwrap_or(true);
        if before_ok && after_ok {
            return Some(absolute);
        }
        start = absolute + 1;
    }
    None
}

/// Curated list. Skewed toward technologies the typical Recall
/// user mentions in saved memories — code snippets, blog posts,
/// product comparisons. Capitalization is canonical (display
/// form).
const WHITELIST: &[&str] = &[
    // Languages
    "Rust",
    "Python",
    "JavaScript",
    "TypeScript",
    "Go",
    "Swift",
    "Kotlin",
    "C++",
    "C#",
    "Ruby",
    "PHP",
    "Java",
    "Scala",
    "Clojure",
    "Elixir",
    "Erlang",
    "Haskell",
    "OCaml",
    "Zig",
    "Nim",
    "Crystal",
    "Lua",
    "Dart",
    "Bash",
    "PowerShell",
    "SQL",
    // Frontend frameworks / runtimes
    "React",
    "Vue",
    "Svelte",
    "SolidJS",
    "Angular",
    "Next.js",
    "Nuxt",
    "Remix",
    "Astro",
    "Vite",
    "Webpack",
    "Rollup",
    "esbuild",
    "Bun",
    "Deno",
    "Node.js",
    "Express",
    "Fastify",
    "Hono",
    // Backend / runtime
    "Tauri",
    "Electron",
    "Flutter",
    "React Native",
    "Capacitor",
    "Ionic",
    "Django",
    "Flask",
    "FastAPI",
    "Rails",
    "Laravel",
    "Spring",
    "Phoenix",
    "Gin",
    "Axum",
    "Actix",
    "Rocket",
    "Tokio",
    "Async-std",
    // Databases / state
    "PostgreSQL",
    "MySQL",
    "SQLite",
    "MariaDB",
    "MongoDB",
    "Redis",
    "Memcached",
    "Cassandra",
    "DynamoDB",
    "Firestore",
    "Realm",
    "CouchDB",
    "Elasticsearch",
    "Meilisearch",
    "Typesense",
    "Algolia",
    "ClickHouse",
    "DuckDB",
    "Snowflake",
    "BigQuery",
    "Redshift",
    "Databricks",
    // ML / AI products
    "Claude",
    "GPT-4",
    "GPT-5",
    "ChatGPT",
    "Gemini",
    "Llama",
    "Mistral",
    "Qwen",
    "PyTorch",
    "TensorFlow",
    "JAX",
    "Hugging Face Transformers",
    "Transformers",
    "Diffusers",
    "LangChain",
    "LlamaIndex",
    "Ollama",
    "vLLM",
    "TGI",
    "candle",
    "llama.cpp",
    // Tools
    "Docker",
    "Kubernetes",
    "Helm",
    "Terraform",
    "Pulumi",
    "Ansible",
    "Chef",
    "Puppet",
    "Vagrant",
    "Git",
    "Mercurial",
    "Make",
    "CMake",
    "Bazel",
    "Cargo",
    "npm",
    "pnpm",
    "yarn",
    "pip",
    "uv",
    "poetry",
    // Editors / IDEs
    "VS Code",
    "Visual Studio",
    "JetBrains",
    "IntelliJ",
    "PyCharm",
    "WebStorm",
    "Vim",
    "Neovim",
    "Emacs",
    "Sublime Text",
    "Zed",
    "Cursor",
    "Atom",
    "Xcode",
    "Android Studio",
    // Comms / collaboration
    "Slack",
    "Discord",
    "Zoom",
    "Teams",
    "Notion",
    "Linear",
    "Jira",
    "Asana",
    "Trello",
    "Confluence",
    "Miro",
    "Figma",
    "Sketch",
    "Excalidraw",
    "Loom",
    // Browsers
    "Chrome",
    "Firefox",
    "Safari",
    "Edge",
    "Brave",
    "Arc",
    "Vivaldi",
    "Opera",
    // Hardware
    "MacBook",
    "iPhone",
    "iPad",
    "AirPods",
    "Vision Pro",
    "Apple Watch",
    "iMac",
    "Mac mini",
    "Mac Studio",
    "Pixel",
    "Galaxy",
    "Surface",
    "ThinkPad",
    "Steam Deck",
    // Recall itself
    "Recall",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_tauri() {
        let entities = detect("Building a Tauri app for Recall.");
        assert!(entities.iter().any(|e| e.entity_value == "Tauri"));
    }

    #[test]
    fn detects_multi_word_product() {
        let entities = detect("Working in VS Code today.");
        assert!(entities.iter().any(|e| e.entity_value == "VS Code"));
    }

    #[test]
    fn rejects_substring_match() {
        // "Goldfish" should not match "Go"
        let entities = detect("Saw a goldfish at the store.");
        assert!(entities.iter().all(|e| e.entity_value != "Go"));
    }

    #[test]
    fn deduplicates_repeated_product() {
        let entities = detect("Rust is fast. Rust has good ergonomics. Rust ships.");
        let count = entities.iter().filter(|e| e.entity_value == "Rust").count();
        assert_eq!(count, 1);
    }
}
