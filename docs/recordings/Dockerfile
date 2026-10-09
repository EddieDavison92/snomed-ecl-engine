# VHS with a Secret Service, so the setup clip's `login` can save a TRUD key.
# scripts/record.sh builds it; the keyring lives only as long as the container.
FROM ghcr.io/charmbracelet/vhs:v0.10.0
# The image's sources name `stable`, which has moved past its bookworm base.
RUN sed -i 's/\bstable\b/bookworm/g' /etc/apt/sources.list.d/*.sources /etc/apt/sources.list 2>/dev/null; \
    apt-get update -qq \
 && apt-get install -y -qq --no-install-recommends dbus-daemon gnome-keyring >/dev/null \
 && rm -rf /var/lib/apt/lists/*
# A private session bus and an unlocked `login` keyring, then VHS. gnome-keyring
# creates no keyring for an empty password, so this throwaway one has a word.
ENTRYPOINT ["dbus-run-session", "--", "sh", "-c", "printf 'recording\\n' | gnome-keyring-daemon --unlock --components=secrets >/dev/null && exec vhs \"$@\"", "vhs"]
