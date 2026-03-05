// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

interface CallerCheck {
    function getCaller() external view returns (address);
    function recordCaller() external;
    function getLastCaller() external view returns (address);
}
