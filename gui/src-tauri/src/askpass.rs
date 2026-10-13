//! git 凭据弹框(对标 IntelliJ `GitHandlerAuthenticationManager` 的 askpass)。
//!
//! git / ssh 需要用户名、密码或 passphrase 时,会执行 `GIT_ASKPASS` / `SSH_ASKPASS`
//! 指向的程序:参数是提示语,stdout 第一行是答案,非零退出视为取消。这里让 GitSail
//! 自身的可执行文件兼任这个程序:
//! 1. 主进程启动 [`AskpassServer`]:在 127.0.0.1 随机端口监听,把 exe 路径、端口和
//!    随机 token 经 `gitcore::set_process_env` 注入之后的每次 git 调用;
//! 2. git 拉起 `GitSail <提示语>`,[`client_main_if_requested`] 发现带着 askpass 环境
//!    变量,就连回主进程交出 token + 提示语,阻塞等答复,打印后退出,不启动 GUI;
//! 3. 主进程校验 token 后经回调交给 UI 弹框,用户填写或取消后 [`AskpassServer::respond`]。

use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher};
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const ENV_PORT: &str = "GITSAIL_ASKPASS_PORT";
const ENV_TOKEN: &str = "GITSAIL_ASKPASS_TOKEN";

/// 用户迟迟不答时的兜底:超时视为取消,git 随即失败退出,不无限挂着。
const ANSWER_TIMEOUT: Duration = Duration::from_secs(300);
/// 读请求的超时与大小上限:防止异常连接长期占住处理线程。
const READ_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_REQUEST: u64 = 64 * 1024;

/// 一次凭据询问,交给 UI 展示。
#[derive(Debug, Clone, serde::Serialize)]
pub struct AskpassRequest {
    pub id: u64,
    /// git / ssh 给出的原始提示语(ssh 的主机指纹确认是多行)。
    pub prompt: String,
    /// 是否按密码处理(输入框遮蔽)。
    pub secret: bool,
}

/// 推给 UI 的事件。
#[derive(Debug, Clone)]
pub enum AskpassEvent {
    /// 新的询问,等待 [`AskpassServer::respond`]。
    Request(AskpassRequest),
    /// 询问已超时作废,UI 应撤下对应弹框。
    Expired(u64),
}

/// 主进程侧的 askpass 服务。
pub struct AskpassServer {
    port: u16,
    token: String,
    next_id: AtomicU64,
    pending: Mutex<HashMap<u64, mpsc::Sender<Option<String>>>>,
}

impl AskpassServer {
    /// 启动监听线程。`on_event` 在处理连接的线程上被调用。
    pub fn start<F>(on_event: F) -> io::Result<Arc<Self>>
    where
        F: Fn(&AskpassServer, AskpassEvent) + Send + Sync + 'static,
    {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let server = Arc::new(AskpassServer {
            port: listener.local_addr()?.port(),
            token: random_token(),
            next_id: AtomicU64::new(1),
            pending: Mutex::new(HashMap::new()),
        });
        let on_event = Arc::new(on_event);
        let srv = server.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let srv = srv.clone();
                let on_event = on_event.clone();
                // 每个连接独立线程:多个 git 进程可能同时询问(如并行更新子仓)。
                std::thread::spawn(move || srv.handle(stream, &*on_event));
            }
        });
        Ok(server)
    }

    /// 注入给 git 的环境变量。`exe` 为 GitSail 自身可执行文件路径。
    pub fn git_env(&self, exe: &str) -> Vec<(String, String)> {
        vec![
            ("GIT_ASKPASS".into(), exe.into()),
            ("SSH_ASKPASS".into(), exe.into()),
            // OpenSSH 8.4+:无论有无终端都走 SSH_ASKPASS(macOS 自带与 Git for Windows 均满足)。
            ("SSH_ASKPASS_REQUIRE".into(), "force".into()),
            (ENV_PORT.into(), self.port.to_string()),
            (ENV_TOKEN.into(), self.token.clone()),
        ]
    }

    /// 回答某次询问;`None` 表示用户取消。询问已超时或不存在时静默忽略。
    pub fn respond(&self, id: u64, answer: Option<String>) {
        if let Some(tx) = self.pending.lock().unwrap().remove(&id) {
            let _ = tx.send(answer);
        }
    }

    fn handle(&self, mut stream: TcpStream, on_event: &dyn Fn(&AskpassServer, AskpassEvent)) {
        let Some(prompt) = self.read_request(&mut stream) else {
            return;
        };
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        self.pending.lock().unwrap().insert(id, tx);
        on_event(
            self,
            AskpassEvent::Request(AskpassRequest {
                id,
                secret: is_secret_prompt(&prompt),
                prompt,
            }),
        );
        let answer = match rx.recv_timeout(ANSWER_TIMEOUT) {
            Ok(answer) => answer,
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                on_event(self, AskpassEvent::Expired(id));
                None
            }
        };
        // 协议:`+答案` 或 `-`(取消);客户端已退出时写失败无妨。
        let reply = match answer {
            Some(a) => format!("+{a}"),
            None => "-".into(),
        };
        let _ = stream.write_all(reply.as_bytes());
    }

    // 请求格式:`<token>\n<提示语>\0`。token 不符或格式异常返回 None(直接断开)。
    fn read_request(&self, stream: &mut TcpStream) -> Option<String> {
        stream.set_read_timeout(Some(READ_TIMEOUT)).ok()?;
        let mut buf = Vec::new();
        let mut limited = stream.take(MAX_REQUEST);
        let mut byte = [0u8; 1];
        loop {
            match limited.read(&mut byte) {
                Ok(1) if byte[0] == 0 => break,
                Ok(1) => buf.push(byte[0]),
                _ => return None, // EOF / 超时 / 超长:不完整的请求
            }
        }
        let text = String::from_utf8(buf).ok()?;
        let (token, prompt) = text.split_once('\n')?;
        (token == self.token).then(|| prompt.to_string())
    }
}

// 密码类提示遮蔽输入;用户名、ssh 主机指纹确认(yes/no)明文。
fn is_secret_prompt(prompt: &str) -> bool {
    let p = prompt.to_ascii_lowercase();
    p.contains("password") || p.contains("passphrase")
}

// 128 位随机 token。std 的 RandomState 以操作系统随机源播种,其 SipHash 输出对外
// 不可预测;仅用于本机回环连接鉴权,不必为此引入随机数依赖。
fn random_token() -> String {
    let word = || {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        );
        h.finish()
    };
    format!("{:016x}{:016x}", word(), word())
}

/// 若本进程是被 git / ssh 当作 askpass 拉起的,完成一次询问并返回退出码;
/// 否则返回 None,照常启动 GUI。须在初始化任何窗口之前调用。
pub fn client_main_if_requested() -> Option<i32> {
    let port = std::env::var(ENV_PORT).ok()?;
    let token = std::env::var(ENV_TOKEN).ok()?;
    let prompt = std::env::args().nth(1).unwrap_or_default();
    Some(match ask(&port, &token, &prompt) {
        Ok(Some(answer)) => {
            // git / ssh 只取第一行。
            println!("{answer}");
            0
        }
        Ok(None) => {
            // 用户取消(或超时作废):留下标记,git 失败时由 gitcore 展示为"已取消身份验证"。
            eprintln!("{}", gitcore::AUTH_CANCELLED_MARKER);
            1
        }
        // 连不上主进程等真实故障:不加标记,保留 git 的原始报错。
        Err(_) => 1,
    })
}

fn ask(port: &str, token: &str, prompt: &str) -> io::Result<Option<String>> {
    let port: u16 = port
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "bad askpass port"))?;
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))?;
    stream.write_all(format!("{token}\n{prompt}\0").as_bytes())?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply)?;
    // 只有明确的 `-` 才是取消;未答复就断开(主进程退出、token 不符)按故障处理。
    match reply.strip_prefix('+') {
        Some(answer) => Ok(Some(answer.to_string())),
        None if reply == "-" => Ok(None),
        None => Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "askpass 服务未答复",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of(server: &AskpassServer) -> (String, String) {
        (server.port.to_string(), server.token.clone())
    }

    #[test]
    fn roundtrip_answers_and_cancels() {
        let server = AskpassServer::start(|srv, ev| {
            if let AskpassEvent::Request(req) = ev {
                let answer = req.prompt.starts_with("Username").then(|| "alice".into());
                srv.respond(req.id, answer);
            }
        })
        .unwrap();
        let (port, token) = env_of(&server);
        assert_eq!(
            ask(&port, &token, "Username for 'https://example.com': ").unwrap(),
            Some("alice".into())
        );
        assert_eq!(
            ask(&port, &token, "Password for 'https://example.com': ").unwrap(),
            None,
            "回调回答 None 即取消"
        );
    }

    #[test]
    fn wrong_token_is_rejected_without_prompting() {
        let prompted = Arc::new(Mutex::new(false));
        let flag = prompted.clone();
        let server = AskpassServer::start(move |_, _| *flag.lock().unwrap() = true).unwrap();
        let (port, _) = env_of(&server);
        assert!(
            ask(&port, "forged", "Password: ").is_err(),
            "token 不符:直接断开,不算用户取消"
        );
        assert!(!*prompted.lock().unwrap(), "token 不符不得弹框");
    }

    #[test]
    fn secret_detection() {
        assert!(is_secret_prompt("Password for 'https://u@github.com': "));
        assert!(is_secret_prompt(
            "Enter passphrase for key '/home/u/.ssh/id_ed25519': "
        ));
        assert!(is_secret_prompt("u@host's password: "));
        assert!(!is_secret_prompt("Username for 'https://github.com': "));
        assert!(!is_secret_prompt(
            "Are you sure you want to continue connecting (yes/no/[fingerprint])? "
        ));
    }

    #[test]
    fn tokens_are_distinct_and_128_bit() {
        let (a, b) = (random_token(), random_token());
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
    }
}
