# Microsoft Store の申請内容 (最小の 1 回目)

Partner Center (日本語の画面) で nubmiddle の **1 回目の申請**に入れる内容です (アプリとゲーム → nubmiddle → 申請の開始)。
Markdown を表示した状態からコピーして、欄に貼っていく想定で書いています。

1 回目は審査に必要な最小限だけを手で入れます。製品の機能・検索用語・日本語の掲載情報・残りのスクリーンショットは、
公開後に Microsoft Store のワークフローで `store/en-us.toml`・`store/ja-jp.toml` から入れます (2 回目の申請)。

## 価格と使用可能状況

- **市場**: すべての市場
- **表示**: 一般公開。Store で見つけられるようにする
- **スケジュール**: できるだけ早く
- **価格**: 無料。試用版なし、セールなし

## プロパティ

- **カテゴリ**: ユーティリティ & ツール
- **プライバシー ポリシー**: 「個人情報を使用する」を選び、次の URL を入れる。nubmiddle は何も保存しないが、
  低レベルのマウスフックは仕組み上すべてのマウスイベントの通知を受け取り、Raw Input でデバイスの ID を読む。
  Store の方針では「個人情報へのアクセス」と見なされうるため。README のプライバシーの節に、していることを正確に書いてある
  - https://github.com/akivajp/nubmiddle#privacy
- **Web サイト**: https://github.com/akivajp/nubmiddle
- **サポートの連絡先情報**: https://github.com/akivajp/nubmiddle/issues
- **製品の公表**: デフォルトのまま
- **システム要件**: Windows 11 のほかは無し。「推奨」に「A TrackPoint-style pointing stick with a middle button
  (e.g. a ThinkPad or a ThinkPad TrackPoint Keyboard)」と書いてもよい

## 年齢別レーティング

- **アンケート**: 「International Age Rating Coalition (IARC) のアンケートへ回答する準備ができています」
- **アプリの種類**: 「その他のすべてのアプリの種類」
- **評価機関から直接取得した評価を使用するか、物理メディアで配布するか**: いいえ
- **その後の質問**: すべて「いいえ」。結果はどの地域でも一番低い区分 (IARC 3+) になるはず

## パッケージ

- リリースのワークフローの成果物 `nubmiddle-msix-x64` の `nubmiddle-vX.Y.Z-x64.msix` と、
  `nubmiddle-msix-arm64` の `nubmiddle-vX.Y.Z-arm64.msix` を上げる (`-test` の付いたものではない)
- デバイス ファミリ: Windows 10/11 デスクトップ (マニフェストでは Windows 11 以上を指定)

## 申請オプション

- **公開の保留オプション**: 「[今すぐ公開] を選択するまで、この提出物を公開しない」(審査に通ってから、自分で公開する)
- **認定の注意書き**: この画面ではなく、リンク先の「追加のテスト情報」のページに、次の英語の文を貼る

  > nubmiddle is a tray (notification area) app without a main window. On start it shows a balloon pointing at its tray icon (in the overflow area behind the ^ next to the clock); the icon's menu has every setting and "Quit". To try it: hold the middle mouse button and move the mouse (or push the TrackPoint): the page under the cursor scrolls and the cursor turns into an arrow; a plain middle click works as usual. It works with any mouse unless devices are chosen in the menu. No account, sign-in or test data is needed.

- **制限付き機能** (`runFullTrust` の理由。**500 文字まで**): 次の英語の文を貼る

  > nubmiddle is a desktop (Win32) app. It needs full trust for a low-level mouse hook (to hold the middle button and turn movement into wheel events with SendInput), Raw Input (to tell which device pressed the button), temporarily replacing the system cursors with an arrow while scrolling (restored afterwards), and a tray icon. It collects no data and makes no network connections.

## ストア登録情報

パッケージのマニフェストが英語 (`en-us`) と日本語 (`ja-jp`) を宣言しているので、**この 2 つの言語はどちらも必須**になる
(Partner Center が自動で 2 つの言語の欄を出す)。どちらも次の 2 つだけ入れる。

| | 英語 (米国) | 日本語 |
| --- | --- | --- |
| **説明** | `store/en-us.toml` の `description` | `store/ja-jp.toml` の `description` |
| **スクリーンショット** (1 枚) | `store/screenshots/en-1-hint.png` | `store/screenshots/ja-1-hint.png` |

説明は、箇条書きの改行が消えないよう、TOML を直接開いてコピーする。

スクリーンショットのキャプション (200 文字まで):

- 英語:

  > nubmiddle runs in the notification area; on start it shows where to find it

- 日本語:

  > nubmiddle は通知領域で動作します。起動するとアイコンの場所を案内します

**スクリーンショットには nubmiddle 自身の画面 (通知・トレイメニュー) だけを使う。** nubmiddle は画面を持たないため、
ブラウザなどをスクロールしている画面は「製品の機能を示していない」として認定に落ちる
(2026-11-12、ポリシー 10.1.1.3 Inaccurate Representation)。

製品の機能・検索用語・著作権・開発元・残りのスクリーンショットは入れない (2 回目に API で入れる)。

Microsoft Store ロゴ (任意):

- **1:1 のボックス アート**: `StoreBoxArt-1080.png` (`assets/make_icons.py` が生成)
- **9:16 のポスター アート**・**タイル アイコン**: 空のまま

## 認定に落ちたとき (2026-11-13、1 回目)

10.1.1.3 (スクリーンショットが製品を示していない) で落ちた。Partner Center の同じ申請を開いて直し、もう一度提出する。

1. **ストア登録情報** (英語・日本語): スクリーンショットを削除し、上の表の `*-1-hint.png` に差し替える (キャプションも上のとおり)
2. **追加のテスト情報**: そのままでよい (製品単位で保存されている。API の申請データの `notesForCertification` は別の欄で、空でも問題ない)
3. 「ストアに提出」

## 公開後

1. 公開されたら、Store ID (製品 ID の画面) を `.github/workflows/msstore.yml` の `app-id` に入れる
2. Azure portal の Entra のアプリ `store-submission` に、このリポジトリのフェデレーション資格情報を足す
   (環境 `msstore`。手順は Vault の「Microsoft Store 登録」)
3. Microsoft Store のワークフローを `submit` (`images` 付き、下書き) で動かし、ログに出た変更を確かめて `commit` する
