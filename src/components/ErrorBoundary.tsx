import { Component, type ReactNode } from "react";

interface Props {
  children: ReactNode;
}

interface State {
  hasError: boolean;
  message: string;
}

/** 全局错误边界：渲染异常兜底（防整窗白屏，显示可恢复提示） */
export default class ErrorBoundary extends Component<Props, State> {
  state: State = { hasError: false, message: "" };

  static getDerivedStateFromError(err: unknown): State {
    return { hasError: true, message: err instanceof Error ? err.message : String(err) };
  }

  handleReload = () => {
    // 整页重载：子树状态可能已损坏，仅重置边界状态无法恢复（状态性错误会复发）
    window.location.reload();
  };

  render() {
    if (this.state.hasError) {
      return (
        <div className="h-screen flex flex-col items-center justify-center gap-3 bg-surface-0 text-text-1">
          <div className="text-[15px] font-medium">界面渲染出错了</div>
          <div className="text-[12px] text-text-muted max-w-md text-center break-all px-6">
            {this.state.message}
          </div>
          <button
            onClick={this.handleReload}
            className="mt-1 px-4 py-2 rounded-lg text-[12px] bg-surface-2 hover:bg-surface-3 border border-border/50 text-text-2 transition-colors"
          >
            重新加载
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}
