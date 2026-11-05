---
name: kotoshiro
description: 環境変数 KOTOSHIRO_TODO が設定されたセッションでの進捗記録規則。TODO の状態確認、journal への記録、checklist の更新、障害 TODO の起票。
---

# kotoshiro: TODO に紐づいたセッションの作法

このセッションは TODO `$KOTOSHIRO_TODO` の作業として起動されている。
進捗の正本は TODO の journal であり、書き手はこのセッション自身。

## 現状を読む

```
kotoshiro show --json
```

title / status / checklist / 直近の journal / digest が返る。作業開始時と、何をすべきか迷った時に読む。

## 区切りごとに note を打つ

```
kotoshiro note "<1〜3 行の要点>"
```

打つタイミング (いずれも必須):

1. 計画が固まった時 (何をどの順でやるか)
2. checklist の 1 項目が終わった時
3. 詰まった時 (何を試して何が分からないか)
4. ユーザーに返答を返す直前 (この時点の状態と次の一手)

内容は後で自分または別セッションが読んで作業を再開できる密度で書く。ユーザー向けの体裁は不要。

## checklist を消す

```
kotoshiro check <n>          # n は show に出る番号 (1 始まり)
kotoshiro check <n> --undo
```

項目が終わったら自分で消してよい。journal にも自動で記録される。

## スコープ外の障害を見つけたら

```
kotoshiro new "<title>" --blocks $KOTOSHIRO_TODO
```

現 TODO は自動的に blocked になる。障害の内容は新 TODO の本文ではなく、現 TODO の note に「何に阻まれているか」を書く。

## やらないこと

- `kotoshiro status <id> done` は打たない。完了判断は人がする
- journal.md / digest.md / sessions.json を直接編集しない。必ず CLI 経由
