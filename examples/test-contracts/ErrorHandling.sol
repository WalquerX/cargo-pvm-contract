// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

interface ErrorHandling {
    function willRevert() external;
    function willSucceed() external view returns (bool);
    function setGuarded(uint256 val) external;
    function getGuarded() external view returns (uint256);
}
