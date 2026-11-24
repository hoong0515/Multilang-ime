# Code signing policy

> Status: releases are currently unsigned. This policy applies to every signed release once a code signing certificate is in place.

## What is signed

Only binaries built from this repository by the GitHub Actions workflow [`.github/workflows/build.yml`](../.github/workflows/build.yml)
from a tagged commit (`v*`):

- `multilang_tip.dll` (64-bit and 32-bit), `backend.exe`, `MultilangIME-CandWin.exe`, `MultilangIME-Settings.exe`
- `mozc\mozc_stdio.exe` (built in the same workflow from the pinned [google/mozc](https://github.com/google/mozc) source and the
  helper sources in [`mozc-helper/`](../mozc-helper/))
- the installer, once there is one

Third-party binaries shipped unchanged (the Microsoft Visual C++ runtime DLLs next to `mozc_stdio.exe`) keep their own signatures
and are not re-signed. Product name and version in the signed files are "Multilang IME" and the release version.

## Team roles

| Role | Members |
|---|---|
| Committers and reviewers | [hoong0515](https://github.com/hoong0515) |
| Approvers | [hoong0515](https://github.com/hoong0515) |

- Changes from people outside the team are merged only after review by a committer.
- Every signing request is approved manually by an approver, for that release only.
- Team members use multi-factor authentication for GitHub and the signing service.

## Privacy policy

This program will not transfer any information to other networked systems unless specifically requested by the user or the person
installing or operating it.

Multilang IME runs on your PC only. Its language model, dictionaries and Japanese conversion engine (Mozc) are local; what you type,
your learning data (`%LOCALAPPDATA%\multilang-ime`) and your user dictionary never leave the PC. The program contains no telemetry,
no update check and no network code.

## System changes and uninstall

The installer asks for administrator rights and lists every change it makes (program folder, TSF registration, scheduled task for
the per-user backend, Start menu shortcut, Programs and Features entry, input method in the language list). It is removed from
Programs and Features or with `uninstall.cmd` in the program folder, which deletes only the files it installed
(see [`installer/README-install.md`](../installer/README-install.md)).

---

# 코드 서명 정책 (요약)

- 이 저장소의 GitHub Actions가 태그(`v*`)에서 빌드한 파일만 서명한다. 릴리스마다 승인자가 직접 승인한다.
- 개인정보: 입력 내용, 학습 기록, 사용자 사전은 PC 밖으로 나가지 않는다. 원격 수집, 업데이트 확인, 네트워크 기능이 없다.
- 현재 상태: 릴리스는 서명되지 않는다. 인증서를 마련하면 이 정책대로 서명한다.
