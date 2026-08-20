# Context

- zdt is a graphical modal text editor
- the idea is to follow modern neovim semantics (unlike other editors like zed or vscode where it is an afterthought or mediocre plugin)
- zdt should still make use of GUI features that don't work well in a terminal to build nice UI where it makes sense (still with keyboard as primary input method)
- zdt should be opinionated and strongly inspired by astronvim by default but stay configurable where it makes sense
- read the 'zgui' and 'zgui-ui' skills for context on the UI framework
- keep performance in mind, the UI should always feel very fast and responsive

# Rust Guidelines

- write idiomatic rust, use the type system (e.g. enum, newtypes, etc.)
- write small, focused modules with clear separation of concerns
- nest modules with dir modules for organisation
- use the mod.rs style
- organize modules by feature, not by kind
- declare dependencies on the workspace level
- make sure to use the latest version of dependencies (unless there is a specific reason not to)

# Documentation and Comment Guidelines

- write very short and concise comments
- follow an ASD-STE100 writing style
- avoid contrastive negations, do not use phrases like:
  - "Not x, but y"
  - "x, instead of y"
  - "Rather x than y"
- never reference external documentation, planning or previous bugs/issues in comments (comment should always describe the current state of the code)