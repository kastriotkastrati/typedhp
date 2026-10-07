#![allow(clippy::needless_return)]

use typedhp::StripError;
use typedhp::strip;

fn stripped(source: &str) -> Result<String, StripError> {
  return strip(source.as_bytes()).map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
}

#[test]
fn passes_plain_php_through_unchanged() {
  let source = r#"<?php
declare(strict_types=1);

namespace App;

use Foo\Bar;
use function strlen;
use const PHP_EOL;

#[Attribute]
final readonly class Point implements \JsonSerializable
{
    public const int ORIGIN = 0;
    const LABEL = 'point';
    private static ?self $cache = null;

    public function __construct(
        #[SensitiveParameter] public int $x,
        private int|float $y = 0,
        protected (\Countable&\ArrayAccess)|null $items = null,
    ) {}

    public function toArray(): array {
        return ['x' => $this->x];
    }

    public function empty(): array{}

    public static function make(int ...$values): static
    {
        $sum = static fn(int $a, int $b): int => $a + $b;
        $call = $this->fn(A < B, C > D);
        $name = Point::class;
        return new static(...$values);
    }

    public function jsonSerialize(): mixed /* comment */ {
        return function &(array &$list) use ($sum): ?array { return $list; };
    }
}
?>
<p><?= strlen("x {$y} <T>") ?></p>
"#;

  assert_eq!(stripped(source), Ok(source.to_string()));
}

#[test]
fn erases_a_generic_function() {
  let source = r#"<?php
function first<T>(list<T> $items): ?T {
    return $items[0] ?? null;
}
"#;

  let expected = r#"<?php
function first(array $items): mixed {
    return $items[0] ?? null;
}
"#;

  assert_eq!(stripped(source), Ok(expected.to_string()));
}

#[test]
fn erases_generic_classes_to_their_bounds() {
  let source = r#"<?php
interface Repository<T> {
    public function find(int $id): ?T;
    public function all(): list<T>;
}

final class Box<T : \Countable> implements Repository<T>, \IteratorAggregate<int, T> {
    private array<int, T> $items = [];

    public function __construct(private T $first) {}

    public function find(int $id): ?T { return $this->items[$id] ?? null; }

    public function all(): list<T> { return $this->items; }

    public function map<U>(callable(T): U $mapper): Box<U> {
        return new Box::<U>($mapper($this->first));
    }

    public function getIterator(): \Traversable<int, T> { yield from $this->items; }
}
"#;

  let expected = r#"<?php
interface Repository {
    public function find(int $id): mixed;
    public function all(): array;
}

final class Box implements Repository, \IteratorAggregate {
    private array $items = [];

    public function __construct(private \Countable $first) {}

    public function find(int $id): ?\Countable { return $this->items[$id] ?? null; }

    public function all(): array { return $this->items; }

    public function map(callable $mapper): Box {
        return new Box($mapper($this->first));
    }

    public function getIterator(): \Traversable { yield from $this->items; }
}
"#;

  assert_eq!(stripped(source), Ok(expected.to_string()));
}

#[test]
fn keeps_every_line_number_when_types_span_several_lines() {
  let source = "<?php\nfunction load<\n    TKey,\n    TValue,\n>(array{\n    id: int,\n    name: NonEmptyString,\n} $row): void {\n    throw new \\Exception('line 9');\n}\n";
  let expected = "<?php\nfunction load\n\n\n(array\n\n\n $row): void {\n    throw new \\Exception('line 9');\n}\n";
  assert_eq!(stripped(source), Ok(expected.to_string()));
}

#[test]
fn scopes_type_params_to_their_closures() {
  let source = r#"<?php
$identity = fn<T>(T $value): T => $value;
$wrap = function<T : \Stringable>(T $value): array<T> {
    $inner = fn(T $again): T => $again;
    return [$inner($value)];
};
$after = fn(T $value): T => $value;
"#;

  let expected = r#"<?php
$identity = fn(mixed $value): mixed => $value;
$wrap = function(\Stringable $value): array {
    $inner = fn(\Stringable $again): \Stringable => $again;
    return [$inner($value)];
};
$after = fn(T $value): T => $value;
"#;

  assert_eq!(stripped(source), Ok(expected.to_string()));
}

#[test]
fn erases_pseudo_types_and_literal_types() {
  let source = r#"<?php
function describe(
    PositiveInt $count,
    ClassString<\Throwable> $class,
    ArrayKey $key,
    'asc'|'desc' $direction,
    int<0, max> $offset,
    NonEmptyList<string>|null $tags,
    \Closure(int): string $format,
    T&\Countable $items,
): NonEmptyString {
    return '';
}
"#;

  let expected = r#"<?php
function describe(
    int $count,
    string $class,
    int|string $key,
    string $direction,
    int $offset,
    ?array $tags,
    \Closure $format,
    T&\Countable $items,
): string {
    return '';
}
"#;

  assert_eq!(stripped(source), Ok(expected.to_string()));
}

#[test]
fn refuses_hyphenated_type_names() {
  let reason = "write this type in TitleCase, such as `NonEmptyString` for `non-empty-string`";
  let parameter = "<?php\nfunction step(\n    int $start,\n    positive-int $step,\n): void {}\n";
  let alias = "<?php\n\ntype Id = non-empty-list<int>;\n";
  let argument = "<?php\n$ids = Ids::<array-key>::make();\n";
  assert_eq!(stripped(parameter), Err(StripError { line: 4, reason }));
  assert_eq!(stripped(alias), Err(StripError { line: 3, reason }));
  assert_eq!(stripped(argument), Err(StripError { line: 2, reason }));
}

#[test]
fn keeps_a_class_the_file_imports_or_declares_under_a_type_name() {
  let source = r#"<?php
namespace App;

use App\Values\PositiveInt;

final class NonEmptyString {}

function label(PositiveInt $count, NonEmptyString $name, NonEmptyList<int> $ids): void {}
"#;

  let expected = r#"<?php
namespace App;

use App\Values\PositiveInt;

final class NonEmptyString {}

function label(PositiveInt $count, NonEmptyString $name, array $ids): void {}
"#;

  assert_eq!(stripped(source), Ok(expected.to_string()));
}

#[test]
fn drops_mixed_parts_of_an_intersection() {
  let source = "<?php\nfunction total<T>(T&\\Countable $items): int { return count($items); }\n";
  let expected = "<?php\nfunction total(\\Countable $items): int { return count($items); }\n";
  assert_eq!(stripped(source), Ok(expected.to_string()));
}

#[test]
fn removes_type_arguments_from_calls() {
  let source = r#"<?php
$box = new Box::<int>(1);
$map = Map::<string, list<int>>::empty();
$value = identity::<string>('x');
$result = $service->load::<User>($id);
"#;

  let expected = r#"<?php
$box = new Box(1);
$map = Map::empty();
$value = identity('x');
$result = $service->load($id);
"#;

  assert_eq!(stripped(source), Ok(expected.to_string()));
}

#[test]
fn erases_property_constant_and_trait_types() {
  let source = r#"<?php
class Settings {
    use Collects<User>;

    public const list<string> NAMES = [];
    private ?array{debug: bool} $flags = null;
    public static NonEmptyString $label = 'x';
}
"#;

  let expected = r#"<?php
class Settings {
    use Collects;

    public const array NAMES = [];
    private ?array $flags = null;
    public static string $label = 'x';
}
"#;

  assert_eq!(stripped(source), Ok(expected.to_string()));
}

#[test]
fn reports_the_line_of_a_broken_type_param_list() {
  let source = "<?php\n\nfunction broken<T(T $x) {}\n";
  let expected = StripError { line: 3, reason: "expected `,` or `>` in a type parameter list" };
  assert_eq!(stripped(source), Err(expected));
}

#[test]
fn reports_the_line_of_a_broken_parameter_type() {
  let source = "<?php\nfunction broken(\n    int $a,\n    list<int $b,\n) {}\n";
  let expected = StripError { line: 4, reason: "cannot read this parameter type" };
  assert_eq!(stripped(source), Err(expected));
}

#[test]
fn erases_type_aliases_declared_in_the_same_file() {
  let source = r#"<?php
namespace App\Types;

type UserId = int;
type Row = array{
    id: UserId,
    status: 'active'|'banned',
};

function find(UserId $id, ?Row $row): UserId|null { return $id; }
"#;

  let expected = "<?php\nnamespace App\\Types;\n\n\n\n\n\n\n\nfunction find(mixed $id, mixed $row): mixed { return $id; }\n";
  assert_eq!(stripped(source), Ok(expected.to_string()));
}

#[test]
fn erases_type_aliases_imported_from_other_files() {
  let source = r#"<?php
namespace App;

use type App\Types\UserId;
use type App\Types\{Email, Status as AccountStatus};
use App\Models\User;

final class Users {
    public function find(UserId $id): ?User { return null; }
    public function invite(Email $email, AccountStatus $status, Status $other): void {}
}
"#;

  let expected = r#"<?php
namespace App;



use App\Models\User;

final class Users {
    public function find(mixed $id): ?User { return null; }
    public function invite(mixed $email, mixed $status, Status $other): void {}
}
"#;

  assert_eq!(stripped(source), Ok(expected.to_string()));
}

#[test]
fn leaves_php_that_uses_the_word_type_alone() {
  let source = r#"<?php
trait type {}
class Doc {
    use type;
    const type = 1;
    const type LABEL = 2;
    public string $type = 'a';
    public function type(): string { return $this->type; }
}
function type(): void {}
type();
$name = type::class;
"#;

  assert_eq!(stripped(source), Ok(source.to_string()));
}

#[test]
fn reports_the_line_of_a_broken_type_alias() {
  let source = "<?php\n\ntype Broken = list<;\n";
  let expected = StripError { line: 3, reason: "cannot read this type alias" };
  assert_eq!(stripped(source), Err(expected));
}

#[test]
fn erases_generic_type_aliases() {
  let source = r#"<?php
type Result<T, E = string> = Ok<T>|Err<E>;
function load(): Result<int> { return new Ok(1); }
function parse(?Result<list<int>, \Throwable> $last): void {}
"#;

  let expected = r#"<?php

function load(): mixed { return new Ok(1); }
function parse(mixed $last): void {}
"#;

  assert_eq!(stripped(source), Ok(expected.to_string()));
}

#[test]
fn reports_a_bound_on_a_type_alias_parameter() {
  let source = "<?php\n\ntype Named<T: \\Stringable> = list<T>;\n";
  let reason = "a type alias parameter cannot have a bound or a variance";
  assert_eq!(stripped(source), Err(StripError { line: 3, reason }));
}
