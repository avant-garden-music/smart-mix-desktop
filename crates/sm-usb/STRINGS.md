# core/usb — i18n keys (EN source strings)

Contract: `from core.i18n import t` → `t("usb.scan.title", n=42)` interpolates
`{n}`. Translation agent: add each key below to
`core/i18n/locales/<lang>.json` as `{"<key>": "<translated>"}`.
**Preserve the `{placeholders}` exactly as written.**

Context notes:
- `{path}` is always a POSIX-style relative path; `{device}` is a device
  node/letter; `{token}` is the double-confirmation token.
- `usb.format.*` messages must keep explaining admin elevation clearly:
  formatting is destructive and may require an elevated prompt.

## usb.scan

- `usb.scan.not_a_dir` — Not a folder or not mounted: {root}
- `usb.scan.start` — Scanning drive {root}
- `usb.scan.progress` — Scanning: {n} files found
- `usb.scan.read_error` — Could not read {path}: {error}
- `usb.scan.title` — Found {n} files ({bytes})

## usb.backup

- `usb.backup.start` — Backing up {n} files to {dest}
- `usb.backup.progress` — Copying {path}
- `usb.backup.done` — Backup complete: {n} files ({bytes})
- `usb.backup.verify_start` — Verifying backup hashes
- `usb.backup.verify_done` — Backup verification finished: {n} mismatches
- `usb.backup.hash_mismatch` — SHA-256 mismatch in backup for {path}
- `usb.backup.missing_file` — Missing from backup: {path}
- `usb.backup.read_error` — Could not back up {path}: {error}

## usb.fat32

- `usb.fat32.oversize` — {path} is {size} bytes; FAT32 files max out at {max} bytes (4 GB). Re-encode to FLAC or ALAC, or split the file.
- `usb.fat32.invalid_chars` — {path} contains characters not allowed on FAT32: {chars}
- `usb.fat32.emoji` — {path} contains emojis or symbols that may not survive a FAT32 format
- `usb.fat32.name_too_long` — {path} has a name longer than 255 characters
- `usb.fat32.name_gt_31` — {path}: name is longer than 31 characters - some FAT32 DJ gear truncates or skips it
- `usb.fat32.loose_track` — {path} is loose in the drive root; moving it into a subfolder is recommended

## usb.format

- `usb.format.bad_device.windows` — Windows device must be a drive letter like E or E: (got "{device}")
- `usb.format.bad_device.macos` — macOS device must be a disk identifier like disk4 or /dev/disk4 (got "{device}")
- `usb.format.bad_device.linux` — Linux device must be a block device like /dev/sdb (got "{device}")
- `usb.format.label_truncated` — Volume label "{label}" was truncated to "{new}" (FAT32 allows 11 characters)
- `usb.format.linux_umount` — Unmount the device first: umount {device}
- `usb.format.windows_large` — The volume is larger than 32 GB: the Windows GUI cannot format FAT32 here. Use the PowerShell command provided (diskpart is the equivalent alternative).
- `usb.format.needs_admin` — Administrator elevation is required: run the command from an elevated prompt (Windows: "Run as administrator"; macOS/Linux: sudo).
- `usb.format.wipe_warning` — WARNING: this ERASES everything on {device}. Only run it after the backup has been verified.
- `usb.format.unsupported_os` — Unsupported operating system: {os}

## usb.restore

- `usb.restore.start` — Restoring {n} files to {dest}
- `usb.restore.progress` — Restoring {path}
- `usb.restore.done` — Restore complete: {n} files
- `usb.restore.hash_mismatch` — SHA-256 mismatch after restore: {path}
- `usb.restore.missing_file` — Could not restore {path}: {error}

## usb.workflow

- `usb.workflow.step_scan` — [1/5] Scanned drive: {n} files
- `usb.workflow.step_validate` — [2/5] FAT32 validation: {n} warnings
- `usb.workflow.step_backup` — [3/5] Backup: {n} files copied
- `usb.workflow.step_verify` — [4/5] Backup verified: {n} hashes OK
- `usb.workflow.step_format` — [5/5] Format prepared: {command}
- `usb.workflow.step_restore` — Restore complete: {n} files back on the drive
- `usb.workflow.dry_run` — Dry run: nothing was written. A real run would back up to {dest}
- `usb.workflow.backup_corrupt` — Backup verification failed with {n} mismatches: aborting before the format
- `usb.workflow.confirmation_missing` — Double confirmation required: type the token {token} TWICE to authorize the format
- `usb.workflow.format_failed` — The format command reported failure; nothing was restored
- `usb.workflow.format_skipped` — Format not executed here: run the prepared command manually, then restore
- `usb.workflow.no_format_device` — No format device given: backup-only run
- `usb.workflow.nothing_to_backup` — The drive has no files to back up

## usb.report

- `usb.report.title` — USB Safe Format — summary
- `usb.report.dry_run_note` — (dry run: counts are from the scan only)
- `usb.report.special` — DJ libraries detected: {names}
- `usb.report.tracks` — Tracks: {restored}/{n} restored
- `usb.report.restored` — Files restored: {n}/{total}
- `usb.report.playlists_cues` — Playlists/cues metadata restored (PIONEER/EngineLibrary): {n}
- `usb.report.bytes` — Total data: {size}
- `usb.report.fat32_warnings` — FAT32 warnings before format: {n}
- `usb.report.errors` — Errors: {n}
- `usb.report.no_errors` — No errors
