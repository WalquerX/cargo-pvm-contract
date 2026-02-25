// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

interface DynamicTypes {
    function getStringLength(string calldata s) external view returns (uint256);
    function echoString() external view returns (string memory);
    function getBytesLength(bytes calldata b) external view returns (uint256);
    function echoBytes() external view returns (bytes memory);
    function sumArray(uint256[] calldata arr) external view returns (uint256);
    function getArray() external view returns (uint256[] memory);
}
