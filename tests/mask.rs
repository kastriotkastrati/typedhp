#![allow(clippy::needless_return)]

use typedhp::StripError;

fn text(bytes: &[u8]) -> String {
  return String::from_utf8_lossy(bytes).into_owned();
}

fn typed_source() -> &'static str {
  return r#"<?php
namespace App;
use type App\Types\{Id,   Name};
type   Pair<T> =   array{T, T} ;
final class Box<T> extends Base<T>
{
    public function __construct(private List<T> $items) {}

    public function map<U>(\Closure(T): U $mapper): Box<U> {
        $pick = fn<V>(V $value): V => $value;
        $keep = function &<W>(W $value): W { return $value; };
        return new Box::<U>([]);
    }
}
"#;
}

#[test]
fn hides_typed_syntax_behind_plain_names_of_the_same_width() -> Result<(), StripError> {
  let masked = typedhp::mask(typed_source().as_bytes())?;
  assert_eq!(
    text(&masked.code),
    r#"<?php
namespace App;
use App\Types\Id\_q6;
_q10;
final class _q0___ extends _q1____
{
    public function __construct(private _q7____ $items) {}

    public function _q2___(_q8___________ $mapper): _q9___ {
        $pick = fn(V $value): V => $value;
        $keep = function &(W $value): W { return $value; };
        return new _q5_____([]);
    }
}
"#
  );

  return Ok(());
}

#[test]
fn restores_the_types_after_the_layout_changes() -> Result<(), StripError> {
  let masked = typedhp::mask(typed_source().as_bytes())?;
  let formatted = r#"<?php

namespace App;

use App\Types\Id\_q6;

_q10;

final class _q0___ extends _q1____
{
  public function __construct(
    private _q7____ $items,
  ) {}

  public function _q2___(_q8___________ $mapper): _q9___
  {
    $pick = fn (V $value): V => $value;
    $keep = function & (W $value): W {
      return $value;
    };

    return new _q5_____([]);
  }
}
"#;

  let restored = typedhp::unmask(formatted.as_bytes(), &masked)?;
  assert_eq!(
    text(&restored),
    r#"<?php

namespace App;

use type App\Types\{Id, Name};

type Pair<T> = array{T, T};

final class Box<T> extends Base<T>
{
  public function __construct(
    private List<T> $items,
  ) {}

  public function map<U>(\Closure(T): U $mapper): Box<U>
  {
    $pick = fn <V>(V $value): V => $value;
    $keep = function & <W>(W $value): W {
      return $value;
    };

    return new Box::<U>([]);
  }
}
"#
  );

  return Ok(());
}

#[test]
fn restores_untouched_code_as_it_was() -> Result<(), StripError> {
  let sources = [
    include_str!("../runtime/loader.php"),
    include_str!("../runtime/Typedhp/Ok.php"),
    include_str!("../runtime/Typedhp/Err.php"),
    include_str!("../runtime/Typedhp/Result.php"),
    include_str!("../runtime/Typedhp/ProcessRun.php"),
    include_str!("../runtime/Typedhp/Stripper.php"),
    include_str!("../runtime/Typedhp/PrimaryScript.php"),
    include_str!("../runtime/Typedhp/StrippingFileWrapper.php"),
  ];

  sources.iter().try_for_each(|source| {
    let masked = typedhp::mask(source.as_bytes())?;
    let restored = typedhp::unmask(&masked.code, &masked)?;
    assert_eq!(text(&restored), *source);
    return Ok(());
  })
}

#[test]
fn reindents_a_type_that_spans_lines() -> Result<(), StripError> {
  let source = "<?php\nfunction rows(array{\n    id: int,\n} $row): void {}\n";
  let masked = typedhp::mask(source.as_bytes())?;
  assert_eq!(text(&masked.code), "<?php\nfunction rows(_q0______________ $row): void {}\n");

  let formatted = "<?php\n\nclass Rows\n{\n    function rows(_q0______________ $row): void {}\n}\n";
  let restored = typedhp::unmask(formatted.as_bytes(), &masked)?;
  assert_eq!(
    text(&restored),
    "<?php\n\nclass Rows\n{\n    function rows(array{\n        id: int,\n    } $row): void {}\n}\n"
  );

  return Ok(());
}

#[test]
fn hides_a_bare_list_type_that_php_reserves() -> Result<(), StripError> {
  let masked = typedhp::mask(b"<?php\nfunction ids(List $ids): List { return $ids; }\n")?;
  assert_eq!(text(&masked.code), "<?php\nfunction ids(_q0_ $ids): _q1_ { return $ids; }\n");
  return Ok(());
}

#[test]
fn picks_placeholder_names_the_source_does_not_use() -> Result<(), StripError> {
  let source = "<?php\n$_q1 = 1;\nfunction pick(List<int> $values): int { return $_q1; }\n";
  let masked = typedhp::mask(source.as_bytes())?;
  assert_eq!(
    text(&masked.code),
    "<?php\n$_q1 = 1;\nfunction pick(_qq0_____ $values): int { return $_q1; }\n"
  );

  return Ok(());
}

#[test]
fn reports_a_placeholder_the_formatter_lost() -> Result<(), StripError> {
  let masked = typedhp::mask(b"<?php\nfunction pick(List<int> $values): void {}\n")?;
  let restored = typedhp::unmask(b"<?php\nfunction pick($values): void {}\n", &masked);
  assert_eq!(
    restored,
    Err(StripError { line: 1, reason: "the formatter changed code that typedhp hid from it" })
  );

  return Ok(());
}
