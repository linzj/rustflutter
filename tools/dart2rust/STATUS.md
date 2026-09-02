# dart2rust 进度

目标:把 `/d/linzjUbuntu2204/gallery_upstream`(真正的 flutter/gallery)翻译成
Rust 跑起来。这需要翻译 gallery 自身的 Dart **和**它依赖的整个 Flutter framework。

尺子是 `bin/census.dart`:对一棵树跑前端,把拒绝原因**按类别归并**后排队。
队头就是下一件该做的事。用法:

    dart run --packages="E:/source/flutter/.dart_tool/package_config.json" \
        tools/dart2rust/bin/census.dart <目录> [--examples]

---

## 第 2 轮:具名参数

`painting/` 普查队头是 **具名参数 103 次**,远超第二名。

### 为什么这件事有静默出错的余地

Rust 没有具名参数,所以 Dart 的具名调用必须压平成位置调用。
**按调用点顺序压平**在绝大多数调用上都是对的——因为大多数人本来就按声明顺序写。
但 Dart 允许 `Rect.fromLTRB(top: a, left: b)`,这时两个参数会**静默互换**。

所以顺序必须来自**被调方的形参表**。先验证这一点可达:
`edge_insets.dart` 的 **130 个调用点,130 个都拿到了被调方的形参表**。

省略的可选参数也必须补上值,因为生成的 Rust 函数每个参数都是位置的:

- 有字面量默认值 → 按源码文本降下来
- 省略且可空 → `None`,这正是 Dart 给它的值
- 其余 → **停下**。编译器猜出来的默认值和上游写的默认值长得一模一样,
  这是最难在后期发现的一类错。

### 测试要能分辨"对"和"看着对"

fixture `testdata/fixtures/named_args.dart` 的调用**故意不按声明顺序写**,
权重取 10 的幂,好让任何错误排列都落不到正确的和上。

这个 fixture 当场抓出一个 bug:`allDefaults()`(一个具名参数都没给)
走了 `named.isEmpty` 的捷径,**三个默认值一个都没填**,生成了 `weigh()`。
捷径本身是错的——被调方已知时,每个参数都要交代。

变异验证,两个都被杀死:

| 变异 | 结果 |
|---|---|
| 具名参数按调用点顺序取 | 2 个测试失败 |
| 恢复 `named.isEmpty` 捷径 | 生成的 Rust 元数不匹配,rustc 拒绝 |

第二个是 rustc 报出来的,不是我的断言——记准确:它是真杀,但杀它的是编译器。

### 结果

`painting/` 普查:**具名参数 103 → 0**,翻译出的成员 268 → 329。
测试 9 个全过。

---

## 当前队头(`painting/`,48 文件 / 91 个公开类)

| 次数 | 要建的东西 |
|---|---|
| 74 | abstract method |
| 65 | `assert` 语句 |
| 62 | 命名构造函数 |
| 41 | 后缀表达式(`!` 非空断言) |
| 40 | `is` |
| 40 | super 调用 |
| 36 | 字符串插值 |
| 13 | 级联 `..` |
| 13 | setter |

**下一步**:队头 74 的 `abstract method` 要先看清楚**它是不是真缺口**——
`double get _x;` 这种既是抽象又是私有,抽象方法本来就该映射成 trait 方法而不是被拒绝,
所以这 74 里可能有相当部分是**记账方式造成的**,不是真要建的东西。
先分清楚,再决定是做它还是跳到 `assert` 语句(65,最便宜,直接映射 `debug_assert!`)
或命名构造函数(62,`EdgeInsets` 的主要阻塞项)。

**这个数字本身要先被信任**——和 `SliverSemantics` 报 0/86 是同一类问题。
