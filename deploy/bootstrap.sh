#!/usr/bin/env bash
# Prepare a fresh Ubuntu/Debian VPS for the Rigel stack.
#
# Run as root once. It creates the non-root deployment user, installs Docker,
# authorizes the supplied public key, enables the firewall, adds swap for the
# CMS build, and disables password/root SSH only after the key is installed.
set -Eeuo pipefail
umask 022
export DEBIAN_FRONTEND=noninteractive
export NEEDRESTART_MODE=a

RIGEL_USER="${RIGEL_USER:-rigel}"
RIGEL_SSH_PUBKEY_FILE="${RIGEL_SSH_PUBKEY_FILE:-/tmp/rigel-vps.pub}"
SWAP_SIZE="${SWAP_SIZE:-4G}"

log() { printf '\n[rigel] %s\n' "$*"; }
fail() { printf '\n[rigel] ERROR: %s\n' "$*" >&2; exit 1; }
trap 'printf "\n[rigel] failed at line %s\n" "$LINENO" >&2' ERR

[ "$(id -u)" -eq 0 ] || fail "run this script as root"
[ -r /etc/os-release ] || fail "cannot read /etc/os-release"
# shellcheck disable=SC1091
. /etc/os-release
case "${ID:-}" in
  ubuntu|debian) ;;
  *) fail "supported on Debian/Ubuntu only, found ${ID:-unknown}" ;;
esac
[ -n "${VERSION_CODENAME:-}" ] || fail "VERSION_CODENAME is missing"
[ -r "$RIGEL_SSH_PUBKEY_FILE" ] || fail "SSH public key not found at $RIGEL_SSH_PUBKEY_FILE"

PUBKEY="$(head -n 1 "$RIGEL_SSH_PUBKEY_FILE" | tr -d '\r' | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')"
[ -n "$PUBKEY" ] || fail "SSH public key file is empty"
printf '%s\n' "$PUBKEY" | grep -qE '^(ssh-ed25519|ssh-rsa|ecdsa-sha2-|sk-ssh-)' || fail "not an SSH public key"
ssh-keygen -l -f "$RIGEL_SSH_PUBKEY_FILE" >/dev/null || fail "SSH public key is invalid"

log "installing base packages"
apt-get update
apt-get install -y --no-install-recommends ca-certificates curl gnupg openssh-server ufw

log "installing Docker from Docker's apt repository"
install -m 0755 -d /etc/apt/keyrings
curl -fsSL "https://download.docker.com/linux/${ID}/gpg" -o /etc/apt/keyrings/docker.asc
chmod a+r /etc/apt/keyrings/docker.asc
printf 'deb [arch=%s signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/%s %s stable\n' \
  "$(dpkg --print-architecture)" "$ID" "$VERSION_CODENAME" \
  > /etc/apt/sources.list.d/docker.list
apt-get update
apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
systemctl enable --now docker

log "creating ${RIGEL_USER} and installing the public key"
if ! id -u "$RIGEL_USER" >/dev/null 2>&1; then
  adduser --disabled-password --gecos "" "$RIGEL_USER"
fi
usermod -aG sudo,docker "$RIGEL_USER"
RIGEL_HOME="$(getent passwd "$RIGEL_USER" | cut -d: -f6)"
RIGEL_GROUP="$(id -gn "$RIGEL_USER")"
[ -n "$RIGEL_HOME" ] || fail "could not resolve the home directory for $RIGEL_USER"
install -d -m 0700 -o "$RIGEL_USER" -g "$RIGEL_GROUP" "$RIGEL_HOME/.ssh"
touch "$RIGEL_HOME/.ssh/authorized_keys"
grep -qxF "$PUBKEY" "$RIGEL_HOME/.ssh/authorized_keys" 2>/dev/null \
  || printf '%s\n' "$PUBKEY" >> "$RIGEL_HOME/.ssh/authorized_keys"
chown "$RIGEL_USER:$RIGEL_GROUP" "$RIGEL_HOME/.ssh/authorized_keys"
chmod 600 "$RIGEL_HOME/.ssh/authorized_keys"

install -d -m 0755 -o "$RIGEL_USER" -g "$RIGEL_GROUP" /opt/rigel /opt/rigel/deploy /opt/rigel/site

log "adding ${SWAP_SIZE} swap for the CMS build"
if ! swapon --show | grep -q .; then
  if [ ! -f /swapfile ]; then
    fallocate -l "$SWAP_SIZE" /swapfile || dd if=/dev/zero of=/swapfile bs=1M count=4096 status=none
    chmod 600 /swapfile
    mkswap /swapfile >/dev/null
  fi
  swapon /swapfile
  grep -q '^/swapfile[[:space:]]' /etc/fstab || printf '/swapfile none swap sw 0 0\n' >> /etc/fstab
  printf 'vm.swappiness=10\n' > /etc/sysctl.d/99-rigel.conf
  sysctl --system >/dev/null
fi

log "enabling the firewall"
ufw default deny incoming
ufw default allow outgoing
ufw allow 22/tcp
ufw allow 80/tcp
ufw allow 443/tcp
ufw --force enable

log "hardening SSH (root login and password authentication off)"
install -d -m 0755 /etc/ssh/sshd_config.d
# OpenSSH uses the first value it reads. The file must sort before cloud-init's
# 50-cloud-init.conf, which otherwise pins PasswordAuthentication to yes.
rm -f /etc/ssh/sshd_config.d/99-rigel-hardening.conf
cat > /etc/ssh/sshd_config.d/00-rigel-hardening.conf <<'EOF'
PermitRootLogin no
PasswordAuthentication no
KbdInteractiveAuthentication no
PubkeyAuthentication yes
EOF
sshd -t
sshd -T | grep -qi '^passwordauthentication no$' || fail "password authentication is still enabled"
sshd -T | grep -qi '^permitrootlogin no$' || fail "root login is still enabled"
if ! systemctl reload ssh 2>/dev/null; then
  systemctl reload sshd
fi

rm -f "$RIGEL_SSH_PUBKEY_FILE"
log "done: use ssh -i <key> ${RIGEL_USER}@<this-host>"
