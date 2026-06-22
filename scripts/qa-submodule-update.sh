#!/usr/bin/env bash
#
# 手动 QA:验证「全部更新」对子仓库的行为(卡20/21,对标 WebStorm)。
#
#   留在原分支(不 detach) · 脏子仓 autostash · detached 同步到记录提交 ·
#   子仓 pull 冲突进 ConflictView 逐个解决后续跑剩余子仓。
#
# 造一个超级仓库 + 两个子仓:
#   suba —— 本地与远程改同一行,「全部更新」时**冲突**(测 ConflictView 暂停/续跑)
#   subb —— 干净,会快进更新(测冲突解决后继续更新剩余子仓)
#
# 用法:
#   scripts/qa-submodule-update.sh          # 搭建,打印要在 GUI 打开的路径
#   scripts/qa-submodule-update.sh clean    # 删除测试仓库
#
# 全程使用隔离的 GIT_CONFIG_GLOBAL,**不改动你的全局 git 配置**。
set -euo pipefail

QA="${TMPDIR:-/tmp}/gg-qa-submodule"

if [[ "${1:-}" == "clean" ]]; then
  rm -rf "$QA"
  echo "已删除 $QA"
  exit 0
fi

rm -rf "$QA"
mkdir -p "$QA"

# 隔离的 git 配置:身份 + 允许 file:// 子模块(CVE-2022-39253 默认禁用)。
cat >"$QA/gitconfig" <<'EOF'
[user]
  email = qa@qa
  name = qa
[protocol "file"]
  allow = always
[init]
  defaultBranch = main
[commit]
  gpgsign = false
EOF
export GIT_CONFIG_GLOBAL="$QA/gitconfig"
export GIT_CONFIG_NOSYSTEM=1

# 造一个带 1 个提交(base)的 bare 远程。$1 = 名字
make_remote() {
  git init -q --bare "$QA/$1.git"
  git clone -q "$QA/$1.git" "$QA/$1-seed"
  echo base >"$QA/$1-seed/f.txt"
  git -C "$QA/$1-seed" add .
  git -C "$QA/$1-seed" commit -qm base
  git -C "$QA/$1-seed" push -q origin main
}

make_remote suba
make_remote subb
git init -q --bare "$QA/super.git"

# 超级仓库 + 两个子模块,并给超级仓库设上游(否则主仓更新会因无 upstream 报错)。
git init -q "$QA/super"
echo super >"$QA/super/readme.txt"
git -C "$QA/super" add .
git -C "$QA/super" commit -qm init
git -C "$QA/super" submodule add -q "$QA/suba.git" suba
git -C "$QA/super" submodule add -q "$QA/subb.git" subb
git -C "$QA/super" commit -qm "add subs"
git -C "$QA/super" remote add origin "$QA/super.git"
git -C "$QA/super" push -qu origin main

# 两个子仓都放到 main 并跟踪 origin/main;各自远程推进一笔(改同一行)。
for s in suba subb; do
  git -C "$QA/super/$s" checkout -qB main --track origin/main
  git clone -q "$QA/$s.git" "$QA/$s-bump"
  echo "remote-$s" >"$QA/$s-bump/f.txt"
  git -C "$QA/$s-bump" add .
  git -C "$QA/$s-bump" commit -qm remote
  git -C "$QA/$s-bump" push -q origin main
done

# suba:本地也改同一行(与远程分叉)→ 更新时冲突;subb 保持干净 → 快进。
echo "local-suba" >"$QA/super/suba/f.txt"
git -C "$QA/super/suba" add .
git -C "$QA/super/suba" commit -qm local

cat <<EOF

✅ 测试仓库就绪。在 GUI 打开超级仓库:
   $QA/super

点击清单(「全部更新」含子仓):
  1. 主仓「已是最新」→ 自动进子仓更新
  2. suba 冲突 → 弹 ConflictView 三栏 + 顶部黄条「子仓库 suba 更新冲突」
  3. 解决 f.txt → 写入 → 点「继续」
  4. 自动续跑 subb → 显示「已更新 1 个提交」
  5. 终态:suba 冲突已解决、subb 已更新;验证留分支:
       git -C $QA/super/suba branch --show-current   # 应为 main
       git -C $QA/super/subb branch --show-current   # 应为 main
  6.(可选)重跑后在 suba 冲突时点「放弃」→ suba 跳过、subb 仍继续

用完清理:  scripts/qa-submodule-update.sh clean
EOF
