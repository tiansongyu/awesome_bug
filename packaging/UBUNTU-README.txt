SDL2 桌面蟑螂（Ubuntu x86_64）

bin/cockroach_overlay
  单只蟑螂版。

bin/cockroach_swarm_20
  20 只蟑螂版；按当前显示器分辨率分区随机生成，大小与速度略有差异。

程序默认速度倍率为 3。按 Ctrl+Alt+Q 可退出。

桌面图标碰撞面向 Ubuntu GNOME X11 + Desktop Icons NG（DING）：
桌面位于前台时，蟑螂只使用躯干碰撞体绕开图标和文字区域，
腿和触角不参与碰撞；拖拽图标时会避开移动碰撞体，即使图标直接
覆盖蟑螂，蟑螂也会继续移动并选择方向脱离。连续受阻时会主动探测
多个方向并选择空隙脱困，不会长时间卡在图标夹角。

运行依赖（Ubuntu / Debian）：
  sudo apt install libsdl2-2.0-0 libpng16-16 libx11-6 libxfixes3 \
    libxrender1 libatspi2.0-0

运行：
  ./bin/cockroach_overlay
  ./bin/cockroach_swarm_20

纯 Wayland 会限制透明覆盖窗口的绝对定位与全局置顶，建议登录
“Ubuntu on Xorg”会话获得完整功能。
