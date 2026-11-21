#!/bin/sh
# Lint Windows-only code from macOS or Linux before pushing.
# Clippy type-checks without linking, so placeholder C and resource compilers that only
# create empty output files let SQLite and GPUI build scripts finish. This proves nothing
# about Windows builds, tests or runtime behavior; Windows CI still owns those.
# Setup once: rustup target add x86_64-pc-windows-msvc
set -eu
tools=$(mktemp -d)
trap 'rm -rf "$tools"' EXIT

cat > "$tools/cc" <<'EOF'
#!/bin/sh
for arg in "$@"; do
  [ "${next:-}" = 1 ] && : > "$arg"; next=
  case "$arg" in -o) next=1 ;; -Fo?*) : > "${arg#-Fo}" ;; -out:*|/OUT:*) : > "${arg#*:}" ;; esac
done
EOF

cat > "$tools/rc" <<'EOF'
#!/bin/sh
case " $* " in *" /? "*) echo "OVERVIEW: Resource Converter"; echo "/no-preprocess"; exit 0 ;; esac
for arg in "$@"; do
  [ "${next:-}" = 1 ] && : > "$arg"; next=
  case "$arg" in /fo) next=1 ;; /fo?*) : > "${arg#/fo}" ;; esac
done
EOF
chmod +x "$tools/cc" "$tools/rc"

env CC_x86_64_pc_windows_msvc="$tools/cc" AR_x86_64_pc_windows_msvc="$tools/cc" \
  RC_x86_64_pc_windows_msvc="$tools/rc" \
  cargo clippy --locked --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings
