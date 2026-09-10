//! Resolve repo / Worktree / branch for a directory by reading `.git` files directly
//! (gitfile -> commondir -> HEAD). No git subprocess on the hot path. OWNER: detection agent.
