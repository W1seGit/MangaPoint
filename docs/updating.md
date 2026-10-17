# Updating MangaPoint by hand

Versions **1.0.1 and earlier** cannot update themselves. Do this once; from **1.1.0**
onward MangaPoint finds updates on its own (**Settings → About → Check for updates**).

Updating never touches your library, settings or engine. They live in their own folder
(see [Your data](../README.md#your-data)), not in the program.

Not sure which version you have? Open **Settings → About**.

Every file below is on the [latest release](https://github.com/W1seGit/MangaPoint/releases/latest)
page, under **Assets**. Each release also has `SHA256SUMS.txt` if you want to check the download.

## Windows

**If you installed with the setup program** (it appears in Apps → Installed apps):

1. Download `MangaPoint-…-windows-x86_64-setup.exe` and run it. It installs over the old
   version without asking for an administrator password, and closes MangaPoint if it is open.
2. Open MangaPoint from the Start menu.

**If you use the portable program:**

1. Close MangaPoint. Windows will not let a running program be replaced.
2. Download `MangaPoint-…-windows-x86_64-portable.exe`.
3. Put it where the old one was, replacing it, and run it.

## macOS

1. Quit MangaPoint.
2. Download the disk image for your Mac: `…-macos-arm64.dmg` for Apple silicon (M1 or newer)
   or `…-macos-x86_64.dmg` for Intel. Apple menu → About This Mac says which you have.
3. Open it and drag **MangaPoint** onto **Applications**. Choose **Replace** when asked.
4. The app is not signed yet, so macOS may say it cannot verify the developer. The quickest
   way past that is one command in Terminal, which asks for no password:
   `xattr -cr /Applications/MangaPoint.app`. Or open the app once, go to System Settings →
   Privacy & Security, scroll to the bottom and press **Open Anyway** (macOS asks for an
   administrator's password). On macOS 14 and older, right-click the app and choose **Open**
   also works.

## Linux

Close MangaPoint first, then use the row that matches how you installed it.

| You installed | Do this |
|---|---|
| The `.deb` (Debian, Ubuntu, Mint) | Download the new `…-linux-x86_64.deb`, then `sudo apt install ./MangaPoint-…-linux-x86_64.deb` |
| The `.rpm` (Fedora, openSUSE, RHEL) | Download the new `…-linux-x86_64.rpm`, then `sudo dnf install ./MangaPoint-…-linux-x86_64.rpm` |
| The plain file (any other Linux) | Download `…-linux-x86_64`, replace the old file with it, and run `chmod +x` on it |

## If something looks wrong

Open MangaPoint and check **Settings → About** for the new version. If your series seem to
be missing, do not reinstall: see [Your data](../README.md#your-data) for where they are kept.
