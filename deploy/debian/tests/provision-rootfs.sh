#!/bin/sh
# Ausschließlich unter unshare --mount --pid --fork --kill-child --mount-proc.
# Aufruf: sh provision-rootfs.sh /var/tmp/volund-j1-provision-JJJJMMTT CARGOCACHE
set -eu
test "$(id -u)" = 0
test "$$" = 1
base=$(realpath -e "$1")
cache=$(realpath -e "$2")
case "$base" in /var/tmp/volund-j1-provision-[0-9][0-9][0-9][0-9][0-9][0-9][0-9][0-9]) ;; *) exit 2 ;; esac
test ! -e "$base/rootfs"
test -f "$base/source.tar"
test -f "$base/web.tar"
test -f "$base/provision-rootfs.py"
test -f "$base/provision-runtime.py"
cd "$base"
apt-get download debootstrap
dpkg-deb -x ./debootstrap_*.deb bootstrap-tool
export DEBOOTSTRAP_DIR="$base/bootstrap-tool/usr/share/debootstrap"
"$base/bootstrap-tool/usr/sbin/debootstrap" --variant=minbase \
  --include=ca-certificates,sudo,python3,iproute2 trixie "$base/rootfs" \
  https://deb.debian.org/debian
guest="$base/rootfs"
printf '#!/bin/sh\nexit 101\n' > "$guest/usr/sbin/policy-rc.d"
chmod 0755 "$guest/usr/sbin/policy-rc.d"
printf 'Isolierte J.1-Provisionierungsprobe\n' > "$guest/etc/volund-j1-probe"
mkdir -p "$guest/opt/source" "$guest/opt/web" "$guest/opt/cargo"
tar -xf source.tar -C "$guest/opt/source"
tar -xf web.tar -C "$guest/opt/web"
cp provision-rootfs.py provision-runtime.py "$guest/opt/"
cp -a "$cache/." "$guest/opt/cargo/"
mount -t proc proc "$guest/proc"
# debootstrap erzeugt im unprivilegierten LXC keine vollständigen Geräte.
# Nur harmlose Standardgeräte einbinden, keine Host-Datenträger freigeben.
mount -t tmpfs -o mode=0755 tmpfs "$guest/dev"
for device in null zero random urandom tty; do
  touch "$guest/dev/$device"
  mount --bind "/dev/$device" "$guest/dev/$device"
done
mkdir "$guest/dev/pts" "$guest/dev/shm"
mount -t devpts -o newinstance,ptmxmode=0666,mode=0620 devpts "$guest/dev/pts"
mount -t tmpfs -o mode=1777 tmpfs "$guest/dev/shm"
ln -s pts/ptmx "$guest/dev/ptmx"
ln -s /proc/self/fd "$guest/dev/fd"
ln -s /proc/self/fd/0 "$guest/dev/stdin"
ln -s /proc/self/fd/1 "$guest/dev/stdout"
ln -s /proc/self/fd/2 "$guest/dev/stderr"
chroot "$guest" /usr/bin/python3 /opt/provision-rootfs.py packages
chroot "$guest" /usr/bin/python3 /opt/provision-rootfs.py build
# Sämtliche Cluster-/App-Prozesse ab hier in eigener Netz- und PID-Umgebung.
unshare --net chroot "$guest" /usr/bin/python3 /opt/provision-rootfs.py install
echo 'PASS: frisches Debian provisioniert, Anwendung geprüft, eigene Dienste beendet'
