; LinkMind Desktop NSIS 安装/卸载钩子
; 在安装和卸载前结束本应用相关进程，避免旧进程占用文件导致
; "Error opening file for writing"（典型场景：docreader 内嵌 Python
; 加载的 _asyncio.pyd 被仍在后台运行的旧版本进程锁定）。

!macro _LinkMindKillProcesses
  DetailPrint "正在结束 LinkMind Desktop 相关进程..."

  ; /T 结束整个进程树（主程序及其拉起的 linkmind-lite.exe、docreader python.exe 等）
  nsExec::Exec 'taskkill /F /T /IM "LinkMind Desktop.exe"'
  nsExec::Exec 'taskkill /F /T /IM "linkmind-lite.exe"'
  nsExec::Exec 'taskkill /F /T /IM "bsk.exe"'
  ; 兼容更名前的旧版本安装
  nsExec::Exec 'taskkill /F /T /IM "WeKnora Desktop.exe"'

  ; docreader 的内嵌 python.exe 不能按映像名杀（会误杀其他 Python），
  ; 按路径特征结束：运行目录含 docreader 的 python.exe，以及安装目录
  ; （默认 "*\LinkMind Desktop"）下的所有进程，覆盖主程序已退出但
  ; 子进程成为孤儿仍在运行的场景。LIKE 模式刻意不含反斜杠
  ; （WQL 对反斜杠转义处理各异），% 可匹配包括 \ 在内的任意字符。
  nsExec::Exec 'wmic process where $\"ExecutablePath like $\'%docreader%python.exe$\'$\" delete'
  nsExec::Exec 'wmic process where $\"ExecutablePath like $\'%LinkMind Desktop%$\'$\" delete'

  ; 给进程退出和文件句柄释放留出时间
  Sleep 800
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro _LinkMindKillProcesses
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro _LinkMindKillProcesses
!macroend
