# typedhp

TypeScript-style types for PHP. Your files stay `.php`, run on the PHP you already have, and get checked by [Mago](https://github.com/carthage-software/mago).

```php
<?php

final class Stack<T>
{
    private List<T> $items = [];

    public function push(T $item): static {
        $this->items[] = $item;
        return $this;
    }

    public function pop(): ?T {
        return array_pop($this->items);
    }
}

$names = new Stack::<string>();
$names->push('ada');
$names->push(42);
```

```
$ php --typecheck
main.php:19: error[invalid-argument]: Invalid argument type for argument #1 of `Stack::push`: expected `string`, but found `int(42)`.
```

## You don't have to change much

- **Your files stay `.php`.** Plain PHP is already valid typedhp, so you can add types one file at a time.
- **No build step, no output folder.** PHP loads your files as it always does. typedhp strips the types from each file as PHP includes it, and caches the result.
- **Errors keep your real paths and line numbers.** Stack traces, `__FILE__` and `__LINE__` point at your source.
- **`vendor/` is never touched.** Composer, autoloading and your dependencies work as before.
- **Laravel works.** Artisan commands, routes, container injection and `artisan test` all run typed code. PHP processes your script starts, such as `artisan test`, strip types too.
- **Any PHP 8.3 or newer works**, including static builds. There is no extension to install.
- **Type checking uses Mago** and your existing `mago.toml`.
- **`mago format` and `mago lint` keep working**, on typed files too, through the `mago` shim.

## Install

You need Rust to build typedhp, and PHP 8.3 or newer on your `PATH`.

```sh
git clone https://github.com/kastriotkastrati/typedhp.git
cd typedhp
cargo build --release
./target/release/typedhp install ~/.typedhp
```

Then put `~/.typedhp/bin` first on your `PATH`, so that `php` and `mago` find the typedhp shims:

```sh
export PATH="$HOME/.typedhp/bin:$PATH"
```

With mise, add the folder under `[env]` in `~/.config/mise/config.toml`, using the full path that `typedhp install` prints:

```toml
[env]
_.path = ["/home/you/.typedhp/bin"]
```

The `php` shim adds two flags, `--strip-types` and `--typecheck`, and passes everything else to the next `php` on your `PATH`, unchanged.

The `mago` shim runs your Mago. In a project with typed files, `mago format`, `mago lint`, `mago analyze` and `mago guard` run on a plain copy of the project in `.typedhp/`, and typedhp brings the results back to your files. Every other command, and every project without typed files, runs Mago unchanged.

For `--typecheck` and the `mago` shim you need Mago itself. typedhp uses the first of these it finds:

1. the program named in `TYPEDHP_MAGO`
2. your project's `vendor/bin/mago` (`composer require --dev carthage-software/mago`)
3. the next `mago` on your `PATH`

## Usage

```sh
php --strip-types main.php           # run a typed script
php --strip-types artisan migrate    # any artisan command
php --strip-types artisan test       # your test suite
php --strip-types vendor/bin/phpunit
php --typecheck                      # check the whole project with Mago
php --typecheck src/users.php        # extra arguments go to `mago analyze`
mago format                          # format typed files with your mago.toml
mago format --check src/Stack.php    # exit with 1 if a file needs formatting
mago lint                            # lint typed files; errors point at your lines
mago lint --fix                      # apply Mago's fixes to typed files
typedhp strip src/Stack.php          # print the plain PHP that PHP runs
php main.php                         # plain php, as before
```

## Examples

### Generics

Functions, closures, classes, interfaces and traits can take type parameters. A parameter can have a bound (`T: \Countable`), a variance (`+T`, `-T`) and a default (`T = int`).

```php
function first<T>(List<T> $items): ?T {
    return $items[0] ?? null;
}

interface Repository<T>
{
    public function find(int $id): ?T;
}

trait Remembers<T>
{
    private ?T $last = null;
}

final class Box<T: \Countable> implements Repository<T>
{
    use Remembers<T>;

    public function __construct(private T $value) {}

    public function find(int $id): ?T {
        return $this->value;
    }

    public function map<U>(callable(T): U $mapper): U {
        return $mapper($this->value);
    }
}

$identity = fn<T>(T $value): T => $value;
```

### Precise types

The types you already write in PHPDoc work in the code itself. PHP checks the plain part at runtime; Mago checks all of it.

PHPDoc names with hyphens are written in TitleCase: `PositiveInt` for `positive-int`, `NonEmptyList<T>` for `non-empty-list<T>`. Lists are `List<T>` and `List{int, string}`, not `list<T>`. typedhp refuses the old spellings. It writes the hyphens back in the docblocks it makes for Mago, and Mago's messages show the TitleCase names again.

A name such as `PositiveInt` still means your own class when the file imports it with `use` or declares it. A class with one of these names that lives in another file of the same namespace needs a `use` line or its full name, such as `\App\PositiveInt`.

```php
function describe(
    PositiveInt $count,
    NonEmptyString $name,
    'asc'|'desc' $direction,
    array{id: int, email: string} $row,
    ClassString<\Throwable> $error,
    \Closure(int): string $format,
): NonEmptyList<string> {
    // ...
}
```

PHP runs this signature:

```php
function describe(
    int $count,
    string $name,
    string $direction,
    array $row,
    string $error,
    \Closure $format,
): array {
```

### Type aliases

Name a type once, and import it where you need it with `use type`. Aliases cost nothing at runtime, and no file needs to be loaded for them.

```php
<?php // src/types.php

namespace App;

type UserId = PositiveInt;
type User = array{id: UserId, email: NonEmptyString};
```

```php
<?php // src/users.php

namespace App;

use type App\{User, UserId};

function find_user(List<User> $users, UserId $id): ?User {
    foreach ($users as $user) {
        if ($user['id'] === $id) {
            return $user;
        }
    }

    return null;
}
```

Aliases can take type parameters, with defaults. Here `Ok` and `Err` are two small classes with `ok`, `data` and `error` properties, like typedhp's own in `runtime/Typedhp/`:

```php
type Result<T, E = string> = Ok<T>|Err<E>;

function parse(string $text): Result<PositiveInt> {
    $number = (int) $text;
    return $number > 0 ? new Ok($number) : new Err('not a number');
}

function doubled(string $text): int {
    $parsed = parse($text);
    if (!$parsed->ok) {
        return 0;
    }

    return $parsed->data * 2;
}
```

After the `if`, the checker knows `$parsed` is an `Ok<PositiveInt>`. Reading `parse($text)->data` without the check is an error, because `data` may be `null`.

### Type arguments

Pass type arguments with `::<...>`, as in Rust:

```php
$names = new Stack::<string>();
$value = identity::<int>(1);
$users = $repository->load::<User>($id);
```

### Laravel

The container injects typed classes as before:

```php
// app/Support/Greeter.php
final readonly class Greeter
{
    public function __construct(private Stack<string> $names) {}

    public function greet(NonEmptyString $name): NonEmptyString {
        $this->names->push($name);
        return "hello {$name}";
    }
}

// routes/web.php
Route::get('/greet', fn(Greeter $greeter): array<string, string> => [
    'greeting' => $greeter->greet('web'),
]);
```

```sh
php --strip-types artisan serve
php --strip-types artisan test
```

`artisan serve` needs one line in your `php.ini`; see [Good to know](#good-to-know).

### See what PHP runs

`typedhp strip` prints the plain PHP for a file. Lines never move, so errors point at the same line in both:

```
$ typedhp strip src/Stack.php
<?php

namespace App;

final class Stack
{
    private array $items = [];

    public function push(mixed $item): static {
        $this->items[] = $item;
        return $this;
    }

    public function pop(): mixed {
        return array_pop($this->items);
    }
}
```

## How it works

`php --strip-types` starts your real `php` with one extra ini file. That file sets `auto_prepend_file` to typedhp's loader, a few small PHP classes. The loader registers itself as PHP's handler for plain files. When PHP includes a file outside `vendor/`, the loader hands it the output of `typedhp strip` instead, from a cache in `~/.typedhp/cache/`. Other reads, such as `file_get_contents`, still return your original file.

`php --typecheck` runs `typedhp check`. It writes a copy of your project to `.typedhp/check/`, where every type becomes plain PHP plus a docblock that Mago understands (`@template`, `@param`, `@return`, `@var`, `@extends`, `@implements`, `@use`). It runs `mago analyze` in that folder and maps each error back to your file and line. `mago lint` and `mago guard` work the same way, in `.typedhp/lint/` and `.typedhp/guard/`. The folders hold their own `.gitignore`, so git ignores them. Open one to see exactly what Mago checked.

With `--fix`, Mago only reports on the copy. Its report says what each fix changes, and typedhp makes the same change at the same spot in your file. It skips, and reports, a fix that would change text typedhp rewrote for Mago: a type it turned into plain PHP, a type parameter it removed, or a docblock it added. `--unsafe`, `--potentially-unsafe` and `--fail-on-remaining` work as they do in Mago.

`mago format` writes a copy to `.typedhp/format/` in which each type Mago can't read becomes a plain name of the same width, such as `_q3____` for `List<T>`. Mago formats the copy as it would any PHP, and typedhp puts your types back in the formatted result. Because a stand-in is as wide as the type it replaces, Mago breaks lines where it would break them with the real types. Type aliases and `use type` imports become stand-in statements too, so Mago sorts the imports and keeps your blank lines.

## Coding agents

[agents-typedhp.md](agents-typedhp.md) teaches a coding agent the type system, the commands and the mistakes to avoid. Copy it into your project and point your `AGENTS.md` or `CLAUDE.md` at it.

## Good to know

- **Run Mago through the `mago` shim.** Mago run any other way, such as `vendor/bin/mago`, stops at the first type. `--dry-run`, `--format-after-fix`, `--staged` and `--stdin-input` do not work in a project with typed files yet. Run `mago format` after `mago lint --fix` instead of passing `--format-after-fix`.
- **Other tools that parse PHP can't read typed files.** `php -l`, Rector, PHP-CS-Fixer and your editor report parse errors on them. Plain files still work with them.
- **`mago format` leaves the text inside a type as you wrote it.** It moves a type that spans several lines as a block.
- **Write types inline, not in docblocks.** typedhp writes the docblocks Mago needs. A docblock you write by hand reaches Mago as it is, so a type alias inside it is an unknown class.
- **Import an alias with `use type` in every file except the one that declares it,** even within the same namespace. Without the import, `UserId` is a class name.
- **PHP checks only the plain part of a type at runtime.** `List<int>` runs as `array`, `T` as its bound or `mixed`, and an alias as `mixed`. The full check happens in `php --typecheck`.
- **`$box = new Box::<string>(...)` acts like a cast.** The checker takes `Box<string>` as the type of `$box`, but does not check the constructor arguments against it. Use it to create empty objects, such as `new Box::<string>()`. Type arguments on any other call are dropped; the checker infers them from the arguments.
- **Default type parameters** (`T = int`) are allowed, but the checker ignores the default.
- **Run `--typecheck` from your project root.**
- **`artisan serve` needs `variables_order = "GPCS"`** in your `php.ini`. Laravel's serve command doesn't pass environment variables that appear in `$_ENV` to the server process, and typedhp's setting travels in one. With `GPCS`, `$_ENV` stays empty and the setting reaches the server.
- **Tested with PHP 8.3, 8.4 and 8.5, on the command line and with `artisan serve`.** php-fpm should work by loading `ini/typedhp.ini` from your install folder, but it hasn't been tested yet.

## Development

```sh
mise install        # PHP 8.5 (a static build) and Mago
mise run test
mise run php-check  # type-check and lint runtime/
mise run php-format # format runtime/
```

typedhp's own PHP, the loader in `runtime/`, is written in typedhp. `typedhp install` strips it as it copies it.
