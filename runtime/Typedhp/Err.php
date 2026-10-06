<?php declare(strict_types=1);

namespace Typedhp;

final readonly class Err<+E>
{
    public false $ok;

    public null $data;

    public function __construct(public E $error) {
        $this->ok = false;
        $this->data = null;
    }
}
