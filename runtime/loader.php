<?php declare(strict_types=1);

/**
 * Installed by `typedhp install`; PHP runs it first through auto_prepend_file in ini/typedhp.ini.
 * It registers Typedhp\StrippingFileWrapper as PHP's handler for plain files. When PHP includes a file outside vendor/,
 * PHP compiles the stripped copy instead, so __FILE__, paths and line numbers stay the original ones.
 * Stripped copies are cached in cache/, named by the hash of the source and of the typedhp binary.
 */

namespace Typedhp;

require __DIR__ . '/Typedhp/Ok.php';
require __DIR__ . '/Typedhp/Err.php';
require __DIR__ . '/Typedhp/Result.php';
require __DIR__ . '/Typedhp/ProcessRun.php';
require __DIR__ . '/Typedhp/Stripper.php';
require __DIR__ . '/Typedhp/PrimaryScript.php';
require __DIR__ . '/Typedhp/StrippingFileWrapper.php';
if (StrippingFileWrapper::enable()) {
    require StrippingFileWrapper::primaryScript();
    exit();
}
