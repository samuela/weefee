nix-shell is used to manage the development environment. Always run build commands in nix-shell.

Prefer declarative design: use react-style constructs and functional programming idioms where possible. Avoid mutation.

You MUST AT ALL TIMES maintain consistency between the UI state and the state of NetworkManager. You WILL NOT UNDER ANY CIRCUMSTANCE optimistically update the app or UI state.

There is no need for release builds unless they are explicitly requested.
