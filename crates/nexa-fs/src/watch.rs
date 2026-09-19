//! **열린 파일의 외부 변경 감지**(nexa-sql docs/58 §2-6 · 사용자 09-19) — OS 와처 없이 **서명 비교 + 내용 해시 확정**.
//!
//! - [`FileSig`] = (크기 · 수정 시각 · 파일 식별자) — stat 한 번. 서명이 같으면 읽지도 않는다.
//! - [`StatWatch`] = 전용 스레드: 요청(경로 + 아는 서명)을 받아 stat → 다르면 **서명이 안정될 때까지** 기다렸다가(쓰는 중인 파일 ·
//!   temp+rename 원자적 저장의 삭제→생성 틈) 읽어서 돌려준다. UI 스레드는 파일을 만지지 않는다(네트워크 드라이브의 stat은 막힐 수 있다).
//! - 경로 기준이라 원자적 저장을 자연히 따라간다(inode에 건 와처가 영구히 낡는 함정이 없다) · 의존 0 · 유휴 비용 0(요청이 없으면 잔다).
//!
//! 언제 확인할지(창 활성화 · 탭 전환 · 저장 직전 · 보이는 탭 폴링)는 호스트의 일이다 — 이 모듈은 "확인해 달라"만 받는다.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

/// 파일 서명 — 셋 중 하나라도 다르면 "바뀌었을 수 있다"(확정은 내용 해시로).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileSig {
    pub len: u64,
    pub mtime: Option<SystemTime>,
    /// (장치, inode) — Unix만. 원자적 저장(새 파일로 바꿔치기)을 크기·시각이 같아도 잡는다.
    pub id: Option<(u64, u64)>,
}

/// 경로의 서명(`None` = 없음 · 읽을 수 없음 · 디렉터리).
#[must_use]
pub fn file_sig(path: &Path) -> Option<FileSig> {
    let m = std::fs::metadata(path).ok()?;
    if !m.is_file() {
        return None;
    }
    #[cfg(unix)]
    let id = {
        use std::os::unix::fs::MetadataExt;
        Some((m.dev(), m.ino()))
    };
    #[cfg(not(unix))]
    let id = None;
    Some(FileSig {
        len: m.len(),
        mtime: m.modified().ok(),
        id,
    })
}

/// 내용 해시(FNV-1a 64) — "서명은 다른데 내용은 같다"(touch · 동기화 클라이언트 · 내가 저장한 것)를 거른다. 보안용이 아니다.
#[must_use]
pub fn content_hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 확인 요청 — `key`는 호스트의 식별자(탭 id 등 · 그대로 돌려준다).
#[derive(Clone, Debug)]
pub struct WatchReq {
    pub key: u64,
    pub path: PathBuf,
    /// 호스트가 아는 마지막 서명(`None` = 없던 파일 · 처음 보는 파일).
    pub known: Option<FileSig>,
}

/// 확인 결과 — **달라졌을 때만** 온다(같으면 아무것도 보내지 않는다).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WatchEvent {
    /// 서명이 달라졌다 — 새 서명 · 내용 · 내용 해시.
    Changed {
        key: u64,
        path: PathBuf,
        sig: FileSig,
        bytes: Vec<u8>,
        hash: u64,
    },
    /// 파일이 없어졌다(안정 시간 뒤에도 없음).
    Missing { key: u64, path: PathBuf },
    /// 서명은 달라졌지만 읽지 못했다(잠김 · 권한) — 다음 확인 때 다시.
    Unreadable {
        key: u64,
        path: PathBuf,
        error: String,
    },
}

/// 서명이 안정될 때까지 기다렸다가 읽는다. 돌려주는 값 = 보낼 사건(`None` = 달라지지 않았다).
fn check_one(req: &WatchReq, settle: Duration, max_bytes: u64) -> Option<WatchEvent> {
    let mut sig = file_sig(&req.path);
    if sig == req.known {
        return None;
    }
    // 안정 대기: 연속 두 번 같은 서명이 나올 때까지(최대 5번 · 없음도 "같음"에 포함 — 원자적 저장의 틈은 곧 메워진다).
    for _ in 0..5 {
        std::thread::sleep(settle);
        let again = file_sig(&req.path);
        if again == sig {
            break;
        }
        sig = again;
    }
    if sig == req.known {
        return None;
    }
    let (key, path) = (req.key, req.path.clone());
    let Some(sig) = sig else {
        return Some(WatchEvent::Missing { key, path });
    };
    if sig.len > max_bytes {
        return Some(WatchEvent::Unreadable {
            key,
            path,
            error: format!("file is larger than {max_bytes} bytes"),
        });
    }
    Some(match std::fs::read(&req.path) {
        Ok(bytes) => {
            let hash = content_hash(&bytes);
            WatchEvent::Changed {
                key,
                path,
                sig,
                bytes,
                hash,
            }
        }
        Err(e) => WatchEvent::Unreadable {
            key,
            path,
            error: e.to_string(),
        },
    })
}

/// 감시 스레드 손잡이 — 떨어뜨리면 스레드가 끝난다.
pub struct StatWatch {
    tx: mpsc::Sender<Vec<WatchReq>>,
    rx: mpsc::Receiver<WatchEvent>,
}

impl std::fmt::Debug for StatWatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StatWatch")
    }
}

impl StatWatch {
    /// `wake` = 사건이 생겼을 때 UI를 깨우는 콜백 · `settle` = 서명 안정 대기 · `max_bytes` = 이보다 큰 파일은 읽지 않는다.
    /// 스레드를 만들지 못하면 `None`(감지 없이 동작 — 저장 직전 확인은 호스트가 동기로 한다).
    #[must_use]
    pub fn spawn(
        wake: Box<dyn Fn() + Send>,
        settle: Duration,
        max_bytes: u64,
    ) -> Option<StatWatch> {
        let (tx, req_rx) = mpsc::channel::<Vec<WatchReq>>();
        let (ev_tx, rx) = mpsc::channel::<WatchEvent>();
        std::thread::Builder::new()
            .name("file-watch".into())
            .spawn(move || {
                while let Ok(mut batch) = req_rx.recv() {
                    // 밀린 요청은 합친다 — 같은 열쇠는 마지막 것만(폴링이 쌓여도 stat은 한 번).
                    while let Ok(more) = req_rx.try_recv() {
                        batch.extend(more);
                    }
                    let mut seen = std::collections::HashSet::new();
                    let mut any = false;
                    for req in batch.iter().rev() {
                        if !seen.insert(req.key) {
                            continue;
                        }
                        if let Some(ev) = check_one(req, settle, max_bytes) {
                            if ev_tx.send(ev).is_err() {
                                return;
                            }
                            any = true;
                        }
                    }
                    if any {
                        wake();
                    }
                }
            })
            .ok()?;
        Some(StatWatch { tx, rx })
    }

    /// 확인 요청(비동기 · 같은 열쇠가 겹치면 스레드가 합친다).
    pub fn check(&self, reqs: Vec<WatchReq>) {
        if !reqs.is_empty() {
            let _ = self.tx.send(reqs);
        }
    }

    /// 도착한 사건을 모두 꺼낸다.
    pub fn poll(&self) -> Vec<WatchEvent> {
        self.rx.try_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("nexa_fs_watch_{}_{name}", std::process::id()))
    }

    fn wait(w: &StatWatch) -> Vec<WatchEvent> {
        for _ in 0..200 {
            let ev = w.poll();
            if !ev.is_empty() {
                return ev;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Vec::new()
    }

    #[test]
    fn sig_and_hash_basics() {
        let p = tmp("sig.txt");
        std::fs::write(&p, b"abc").unwrap();
        let s = file_sig(&p).unwrap();
        assert_eq!(s.len, 3);
        assert_eq!(file_sig(&p), Some(s), "같은 파일 = 같은 서명");
        assert_eq!(file_sig(&tmp("nope.txt")), None);
        assert_eq!(
            file_sig(&std::env::temp_dir()),
            None,
            "디렉터리는 파일이 아니다"
        );
        assert_eq!(content_hash(b"abc"), content_hash(b"abc"));
        assert_ne!(content_hash(b"abc"), content_hash(b"abd"));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn watch_reports_only_differences() {
        let p = tmp("watch.txt");
        std::fs::write(&p, b"one").unwrap();
        let known = file_sig(&p);
        let w = StatWatch::spawn(Box::new(|| {}), Duration::from_millis(10), 1 << 20).unwrap();
        let req = |known| {
            vec![WatchReq {
                key: 7,
                path: p.clone(),
                known,
            }]
        };
        // 같음 = 사건 없음.
        w.check(req(known));
        std::thread::sleep(Duration::from_millis(80));
        assert!(w.poll().is_empty());
        // 내용이 바뀜(크기가 달라 mtime 해상도와 무관).
        std::fs::write(&p, b"two!!").unwrap();
        w.check(req(known));
        let ev = wait(&w);
        match &ev[..] {
            [WatchEvent::Changed {
                key,
                bytes,
                hash,
                sig,
                ..
            }] => {
                assert_eq!((*key, bytes.as_slice(), sig.len), (7, &b"two!!"[..], 5));
                assert_eq!(*hash, content_hash(b"two!!"));
            }
            other => panic!("{other:?}"),
        }
        // 삭제.
        std::fs::remove_file(&p).unwrap();
        w.check(req(known));
        assert!(matches!(
            &wait(&w)[..],
            [WatchEvent::Missing { key: 7, .. }]
        ));
        // 없던 파일이 계속 없음 = 사건 없음 · 상한보다 큰 파일 = 읽지 않음.
        w.check(req(None));
        std::thread::sleep(Duration::from_millis(80));
        assert!(w.poll().is_empty());
        std::fs::write(&p, vec![b'x'; 64]).unwrap();
        let small = StatWatch::spawn(Box::new(|| {}), Duration::from_millis(10), 16).unwrap();
        small.check(req(None));
        assert!(matches!(&wait(&small)[..], [WatchEvent::Unreadable { .. }]));
        let _ = std::fs::remove_file(&p);
    }
}
