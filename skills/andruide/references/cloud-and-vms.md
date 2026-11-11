# Cloud machines and VMs

## Cloud machines (DigitalOcean)

AndruIDE can create DigitalOcean droplets in the user's account and add them to their machines. The user signs in from the app (Settings › Cloud) or with `andruide cloud login`; don't handle their token.

```sh
andruide cloud options --json                          # the sign-in, regions, sizes with hourly and monthly prices, the defaults
andruide cloud create dev-1 --region nyc3 --size s-2vcpu-4gb --json
andruide cloud create dev-1 --install docker,gh,node,claude,codex,tailscale --json
andruide cloud usage --json                            # each one's rate, this month, since created, the account's bill
andruide cloud power dev-1 off --json
andruide cloud destroy dev-1 --json
```

- **It costs money every hour it exists.** Before `cloud create`, tell the user the size and its price and get their OK.
- `cloud create` sets it up (the user's account and SSH key, and what `--install` lists, or the user's settings) and adds it to their machines. It waits until that's done.
- A powered-off droplet is still billed. Only `cloud destroy` stops the charges, and it deletes the droplet with everything on it: confirm first, and make sure work there is pushed.
- A cloud machine's `cloud` field in `andruide machine dev-1 --json` has its region, size, price, cost so far and how long it has been idle.

## VMs on the user's machines

AndruIDE can make a VM on one of the user's machines and add it to their machines, shown under that machine:

- Windows (11, 10, Server 2025 or 2022) in Docker with KVM, on a Linux x86-64 machine.
- Linux (Ubuntu, Debian, Fedora, Alpine, Arch) as a container with ssh, on any machine with Docker.

AndruIDE sets up ssh and its agent there, so sessions, files, workspaces and the desktop work like on any machine.

```sh
andruide vm options --json                                         # which machines can run which systems, and the defaults
andruide vm create scratch --system ubuntu-24.04 --json            # on the machine with the most free memory that can
andruide vm create win-test --system windows-11 --on beelink --cpus 4 --memory 8 --disk 64 --json
andruide power win-test off --json                                 # Windows gets two minutes to shut down cleanly
andruide power win-test on --json
andruide vm destroy win-test --json                                # its container, its disk, and the machine in AndruIDE
```

- Systems: `windows-11`, `windows-10`, `windows-server-2025`, `windows-server-2022`, `ubuntu-26.04`, `ubuntu-24.04`, `debian-13`, `fedora`, `alpine`, `arch`. `--install claude,codex` puts agent CLIs on a Linux VM.
- Linux is ready in under a minute. Windows downloads and installs itself: 15-30 minutes the first time. Run that in the background of your shell tool and let it wait. Until it answers, its `unreachable.kind` is `installing`.
- It uses the user's machine, memory and disk. Make one when asked, or when a task needs a fresh machine (a clean Windows to test an installer on). Destroy it when the task is done, unless the user wants it kept; copy out anything worth keeping first.
- A machine's `vm` field says AndruIDE made it, and so can destroy it.
- A Windows VM's password is in the user's Keychain. You don't need it: ssh uses a key.

### "Test this on a clean Windows machine"

1. `andruide vm options --json`: find a machine that lists `windows-11`. None? Windows VMs need a Linux machine (x86-64) with Docker and KVM; tell the user.
2. `andruide vm create win-test --system windows-11 --json`, in the background; it takes 15-30 minutes.
3. Work there: `andruide ws create --machine win-test --json` and `andruide ws exec`, or `andruide cp` and `andruide session start`.
4. `andruide vm destroy win-test --json` when you're done, unless the user wants it kept.
