// Tauri 本地机器身份：单机版永远是本机。桌面版要从 daemonAPI 读 daemonId /
// 主机名并处理 daemon 停止时的身份缓存；Tauri 版 daemon 在进程内（Rust），
// 没有独立 daemon 进程需要追踪，所以用常量。
export const LOCAL_DAEMON_ID = "local";
export const LOCAL_MACHINE_NAME = "本机";
