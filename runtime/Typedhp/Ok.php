<?php declare(strict_types=1);

namespace Typedhp;

final readonly class Ok<+T>
{
    public true $ok;

    public null $error;

    public function __construct(public T $data) {
        $this->ok = true;
        $this->error = null;
    }
}
