// 独立冲突窗口路由:预渲染出静态壳,确保生产包(Tauri 静态资源协议,无 SPA 回退)
// 也能直接加载 /conflict;运行时逻辑仍在 onMount 客户端执行(ssr=false 由 layout 提供)。
export const prerender = true;
