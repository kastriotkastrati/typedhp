<?php declare(strict_types=1);

namespace Typedhp;

use Closure;
use RuntimeException;
use Throwable;

type Result<T, E = null> = Ok<T>|Err<E>;

final class Result
{
    public static function from<T>(Closure(): T $operation): Result<T, Throwable> {
        try {
            $value = $operation();
            return new Ok($value);
        } catch (Throwable $error) {
            return new Err($error);
        }
    }

    public static function okOrThrow<T>(Result<T, NonEmptyString> $result): T {
        if (!$result->ok) {
            throw new RuntimeException($result->error);
        }

        return $result->data;
    }
}
