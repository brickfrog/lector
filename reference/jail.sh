#!/usr/bin/env bash
# Sandbox primitives for the audiobook pipeline.
#
# Two confinement zones (see threat model):
#   offline_jail : NO network. For stages touching untrusted/derived BINARY data
#                  (PDF extract, torchaudio decode, ffmpeg). A parser exploit here
#                  cannot phone home.
#   online_jail  : network ON, but ONLY sees text/cache dirs + the one pass secret.
#                  Never sees the raw ebook.
#
# Both: clean env, read-only system, minimal /dev, private /tmp, die-with-parent.

set -euo pipefail

WORK="${WORK:-$PWD}"
UV_CACHE="${UV_CACHE_DIR:-/mnt/data/.cache/uv}"
TORCH_CACHE="$HOME/.cache/torch"
HF_CACHE="$HOME/.cache/huggingface"

_common_binds=(
  --die-with-parent
  --clearenv
  --setenv PATH "/usr/bin:/usr/local/bin"
  --setenv HOME "$WORK"
  --setenv MINIMAX_MODEL "${MINIMAX_MODEL:-speech-2.8-hd}"
  --setenv KOKORO_VOICE "${KOKORO_VOICE:-bm_george}"
  --setenv UV_CACHE_DIR "$UV_CACHE"
  --setenv UV_OFFLINE "1"
  --setenv TORCH_HOME "$WORK/.torch"
  --setenv XDG_CACHE_HOME "$WORK/.xdgcache"
  --setenv HF_HOME "$WORK/.xdgcache/huggingface"
  --ro-bind /usr /usr
  --ro-bind /etc /etc
  --symlink usr/lib /lib
  --symlink usr/lib /lib64
  --symlink usr/bin /bin
  --symlink usr/bin /sbin
  --proc /proc
  --dev /dev
  --tmpfs /tmp
  # uv cache: real env as RO lower, writes (locks/temp) to throwaway tmpfs upper.
  # An exploited stage cannot poison the cache that unjailed runs reuse.
  --overlay-src "$UV_CACHE"
  --tmp-overlay "$UV_CACHE"
  # uv's managed Python lives here and its absolute path is baked into cached
  # venvs; without it uv judges every cached env invalid and tries to re-resolve.
  --ro-bind-try "$HOME/.local/share/uv" "$HOME/.local/share/uv"
)

# offline: --unshare-all severs net too; bind torch/hf model caches read-only.
offline_jail() {
  bwrap \
    --unshare-all \
    "${_common_binds[@]}" \
    --bind "$WORK" "$WORK" \
    --ro-bind-try "$TORCH_CACHE/hub" "$WORK/.torch/hub" \
    --ro-bind-try "$HF_CACHE" "$WORK/.xdgcache/huggingface" \
    --chdir "$WORK" \
    -- "$@"
}

# online: keep net, but FS = code (ro) + text/caches/audio (rw) only. The raw PDF
# is masked to /dev/null so even a buggy stage can't read the ebook here. Secrets
# arrive as env (caller exports MINIMAX_*); claude creds bound ro for cold-cache
# repair. No --clearenv wipe of the secret: we re-inject via --setenv.
online_jail() {
  bwrap \
    --unshare-user --unshare-ipc --unshare-pid --unshare-uts --unshare-cgroup \
    --share-net \
    "${_common_binds[@]}" \
    --setenv MINIMAX_API_KEY "${MINIMAX_API_KEY:-}" \
    --setenv MINIMAX_GROUP_ID "${MINIMAX_GROUP_ID:-}" \
    --ro-bind "$(readlink -f /etc/resolv.conf)" "$(readlink -f /etc/resolv.conf)" \
    --ro-bind "$WORK" "$WORK" \
    --bind "$WORK/text" "$WORK/text" \
    --bind "$WORK/.repair_cache" "$WORK/.repair_cache" \
    --bind "$WORK/.tts_cache" "$WORK/.tts_cache" \
    --bind "$WORK/audio" "$WORK/audio" \
    --bind /dev/null "$WORK/book.pdf" \
    --chdir "$WORK" \
    -- "$@"
}
# NOTE: cold-cache repair (claude -p) is NOT supported inside online_jail yet —
# would need the claude binary + node + creds bound in. run.sh runs repair in
# offline_jail off the warm cache; a brand-new chapter must be repair-warmed
# first (run stage_repair unjailed once) until that plumbing lands.

# allow sourcing or direct dispatch:  ./jail.sh offline_jail <cmd...>
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  fn="$1"; shift
  "$fn" "$@"
fi
