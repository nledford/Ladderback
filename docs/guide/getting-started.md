# Getting started

This page takes you from a fresh install to a first backup that runs on its own, and then proves the backup works by restoring a file from it. Everything happens in the web UI. Set aside about ten minutes, plus the time the first backup takes.

You need:

- Ladderback [installed and started](install.md), and its web UI open in your browser at `http://127.0.0.1:9898`;
- a folder to keep the backups in, the **repository**: here a folder on this computer, ideally on another disk, such as an external drive;
- on macOS, [Full Disk Access](install.md#macos-full-disk-access) granted to Ladderback, so that it can read every folder you back up.

The links at the top of every page lead to the pages this guide names: Operations, Plans, Snapshots, Repositories and Settings.

## 1. Create a repository

A repository holds the backups, encrypted with a password you choose. Open the **Repositories** page and fill in **Test, connect or create a repository**:

1. **Name:** a short name for it, in lower-case letters, digits and hyphens, such as `external-drive`. Your plans use this name.
2. **Location:** the full path of the folder, such as `/Volumes/Backup/ladderback` for an external drive on a Mac or `/media/demo/backup/ladderback` on Linux. Leave **Hot storage (optional)** empty.
3. **Password:** the repository's password. Without it nobody can read the backups, Ladderback and you included, so keep a copy somewhere safe, such as your password manager.
4. **Where to keep the password:** on a Mac, **In the system keyring**. On Linux, **In the configuration file**: once [started as a service](install.md), Ladderback runs before you log in, and can't read the keyring until you do, so a backup due before then would fail. The Repositories page marks such a password **Kept in the configuration file**. A password can [live in other places](secrets.md) too.
5. Press **Create the repository**. The page says **The repository was created.** and lists the repository.

A repository you create here is maintained from this machine: this machine forgets and prunes its old snapshots, as step 5 shows. If the folder already holds a repository, from rustic or from Ladderback on another computer, press **Connect the repository** instead. Turn on **Maintain this repository from this machine** only when no other machine maintains it. Left off, this machine backs up into the repository but never forgets, prunes or checks it, and its Snapshots page says so; **Maintain from this machine** there turns it on later.

## 2. Add a plan

A plan says what to back up, into which repository, when, and which backups to keep. Open the **Plans** page and press **Add a plan**. The **New plan** form asks:

1. **Name:** the plan's name, such as `documents`. It labels the plan's backups and can't be changed once the plan is saved.
2. **Repository:** the name of the repository from step 1. The form fills in the first repository; check it is the one you want.
3. **What to back up:** in **Folders to back up**, the full path of each folder, one a line, such as `/Users/demo/Documents` on a Mac or `/home/demo/Documents` on Linux. The other fields leave things out: in **Leave out folders holding**, `CACHEDIR.TAG` skips the caches that mark themselves with that file.
4. **When to back up:** a **Cron schedule** of five fields: minute, hour, day of the month, month and day of the week, where `*` means every one. `0 * * * *` backs up every hour on the hour, and `0 3 * * *` each night at 3:00. Left empty, the plan runs only when you start it.
5. **Which snapshots to keep:** each backup is a **snapshot**. **Keep one a day for** `7`, **Keep one a week for** `4` and **Keep one a month for** `12` keep the newest snapshot of each of the last 7 days, 4 weeks and 12 months. Leave the fields you don't need empty.

Leave **Commands to run** empty, and press **Save the plan**. The page says **The plan documents is saved.** and lists the plan, with its **Next run**.

## 3. Run it now

Open the **Operations** page and press **Start backup of documents**. The page lists the backup as it runs, then its **Status**, **Succeeded**, and its **Result**: the snapshot's short ID, such as `7fddec8d`, how many files it read and how much it added to the repository. The first backup reads everything; later ones add only what changed, and are much quicker.

The Operations page is the history: every backup, restore and maintenance run, newest first. A backup that fails says why here.

## 4. Check the next scheduled run

The start page, behind the **Ladderback** link at the top, and the **Plans** page list each plan with its **Next run**: with `0 * * * *`, the next full hour. Ladderback backs up at that time without you, as long as it is running. A run the computer slept through runs once it wakes.

## 5. Restore a file

A backup you have never restored from is only a promise, so restore one file now:

1. Open the **Snapshots** page and press **Snapshots of external-drive**, your repository's name. It lists the snapshots under **Plan documents**, newest first.
2. Choose the snapshot's ID. Its page shows **Folders and files**, the tree of what it holds, from the top of the disk: `Users` on a Mac, `home` on Linux. Open the folders down to a file, and select it.
3. Press **Restore** followed by the file's name. In the dialog, keep **Where to restore** at **Into a new folder**, type the snapshot's short ID the dialog shows into **Snapshot ID**, or press **Copy the ID** and paste it, and press **Restore**.
4. The page says **Restore of** the file **finished. Its files are in** a new folder inside `ladderback-restore` in your home folder, named after the computer, the snapshot and the time. Open the file there and compare it with the original.

A restore into a new folder never touches the original files, and no restore deletes a file. Delete the restored copy once you have checked it.

The same **Snapshots** page has the repository's maintenance. **Forget snapshots** shows which snapshots your plan's keep policy would remove, and removes nothing until you confirm. Forgetting frees no disk space by itself: **Prune unused data** does, by marking the data no snapshot uses any more, which a later prune deletes after the wait it names.

## 6. Turn sign-in on

Until it has an admin password, Ladderback asks nobody to sign in: anyone using this computer can open it, and the start page says so. Open **Settings**, then its **Admin password** tab. Type a password of at least 8 characters into **New password** and **New password again**, and press **Set the admin password**. The page says **Admin password set. Restart Ladderback to turn sign-in on.**

Open the **Restart Ladderback** tab, press **Restart Ladderback**, then **Restart**. From then on the web UI asks for the password. [Sign-in and remote access](remote-access.md) says more, including how to reach Ladderback from another computer.

## Known limits

What the web UI can't do yet:

- **Forget and prune don't run on a schedule yet.** Your plan's keep policy takes effect when you press **Forget snapshots** on the Snapshots page, and the space comes back when you press **Prune unused data**, then again after the wait it names. Until you do, the repository only grows.
- **Restores into a new folder go into `ladderback-restore` in your home folder.** The web UI can't choose another folder; `restore-folder` in the `[agent]` table of `config.toml` can ([technical design §6.1](https://github.com/nledford/Ladderback/blob/main/docs/design/scheduling-and-config.md#61-source-of-truth)).
