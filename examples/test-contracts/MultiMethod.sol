// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

interface MultiMethod {
    function add(uint256 a, uint256 b) external view returns (uint256);
    function mul(uint256 a, uint256 b) external view returns (uint256);
    function isZero(uint256 val) external view returns (bool);
    function getCounter() external view returns (uint256);
    function increment() external;
    function reset() external;
}
