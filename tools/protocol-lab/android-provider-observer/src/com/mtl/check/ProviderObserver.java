// audience: external
// # provider-observer
// 此 Provider 记录 CN 客户端对 com.mtl.check.DataContentProvider 的调用形状.
// 它只记录调用元数据和 Cursor 访问形状, 不读取字段值, 并返回无列无行的合成 Cursor.
package com.mtl.check;

import android.content.ContentProvider;
import android.content.ContentValues;
import android.database.Cursor;
import android.database.CursorWrapper;
import android.database.MatrixCursor;
import android.net.Uri;
import android.os.CancellationSignal;
import android.os.Bundle;
import android.util.Log;

public final class ProviderObserver extends ContentProvider {
    private static final String TAG = "StarpointProviderObserver";

    // //// 初始化观察器 Provider [@x380kkm 2026-07-28] ////
    @Override
    public boolean onCreate() {
        Log.i(TAG, "provider created");
        return true;
    }
    // //// /初始化观察器 Provider ////

    // //// 记录 ContentResolver.call 的存在形状 [@x380kkm 2026-08-03] ////
    @Override
    public Bundle call(String method, String arg, Bundle extras) {
        Log.i(TAG, "call methodPresent=" + (method != null)
                + " argPresent=" + (arg != null)
                + " extrasPresent=" + (extras != null));
        return null;
    }
    // //// /记录 ContentResolver.call 的存在形状 ////

    // //// 记录旧式 query 调用的字段形状 [@x380kkm 2026-07-28] ////
    @Override
    public Cursor query(Uri uri, String[] projection, String selection, String[] selectionArgs, String sortOrder) {
        Log.i(TAG, "query variant=legacy authority=" + authority(uri)
                + " projectionCount=" + arraySize(projection)
                + " selectionPresent=" + (selection != null)
                + " selectionArgumentCount=" + arraySize(selectionArgs)
                + " sortPresent=" + (sortOrder != null));
        return syntheticCursor();
    }
    // //// /记录旧式 query 调用的字段形状 ////

    // //// 记录新式 query 调用的参数存在形状 [@x380kkm 2026-08-03] ////
    @Override
    public Cursor query(Uri uri, String[] projection, Bundle queryArgs, CancellationSignal cancellationSignal) {
        Log.i(TAG, "query variant=modern authority=" + authority(uri)
                + " projectionCount=" + arraySize(projection)
                + " queryArgumentsPresent=" + (queryArgs != null)
                + " cancellationPresent=" + (cancellationSignal != null));
        return syntheticCursor();
    }
    // //// /记录新式 query 调用的参数存在形状 ////

    // //// 记录 MIME 类型探测的 authority [@x380kkm 2026-08-03] ////
    @Override
    public String getType(Uri uri) {
        Log.i(TAG, "getType authority=" + authority(uri));
        return null;
    }
    // //// /记录 MIME 类型探测的 authority ////

    // //// 记录插入调用的存在形状 [@x380kkm 2026-08-03] ////
    @Override
    public Uri insert(Uri uri, ContentValues values) {
        Log.i(TAG, "insert authority=" + authority(uri)
                + " valuesPresent=" + (values != null));
        return null;
    }
    // //// /记录插入调用的存在形状 ////

    // //// 记录删除调用的字段形状 [@x380kkm 2026-08-03] ////
    @Override
    public int delete(Uri uri, String selection, String[] selectionArgs) {
        Log.i(TAG, "delete authority=" + authority(uri)
                + " selectionPresent=" + (selection != null)
                + " selectionArgumentCount=" + arraySize(selectionArgs));
        return 0;
    }
    // //// /记录删除调用的字段形状 ////

    // //// 记录更新调用的字段形状 [@x380kkm 2026-08-03] ////
    @Override
    public int update(Uri uri, ContentValues values, String selection, String[] selectionArgs) {
        Log.i(TAG, "update authority=" + authority(uri)
                + " valuesPresent=" + (values != null)
                + " selectionPresent=" + (selection != null)
                + " selectionArgumentCount=" + arraySize(selectionArgs));
        return 0;
    }
    // //// /记录更新调用的字段形状 ////

    // //// 返回无列无行的合成 Cursor [@x380kkm 2026-08-03] ////
    private static Cursor syntheticCursor() {
        Log.i(TAG, "cursor synthetic=true columnCount=0 rowCount=0");
        return new ShapeCursor();
    }
    // //// /返回无列无行的合成 Cursor ////

    // //// 记录 Cursor 列和访问顺序但不返回字段值 [@x380kkm 2026-08-03] ////
    private static final class ShapeCursor extends CursorWrapper {
        ShapeCursor() {
            super(new MatrixCursor(new String[0]));
        }

        @Override
        public String[] getColumnNames() {
            String[] names = super.getColumnNames();
            Log.i(TAG, "cursor columns count=" + names.length);
            return names;
        }

        @Override
        public int getColumnIndex(String columnName) {
            int index = super.getColumnIndex(columnName);
            Log.i(TAG, "cursor columnIndex name=" + safeText(columnName) + " index=" + index);
            return index;
        }

        @Override
        public int getColumnIndexOrThrow(String columnName) {
            int index = super.getColumnIndex(columnName);
            Log.i(TAG, "cursor columnIndexOrThrow name=" + safeText(columnName) + " index=" + index);
            return index;
        }

        @Override
        public int getCount() {
            Log.i(TAG, "cursor getCount=0");
            return 0;
        }

        @Override
        public boolean moveToFirst() {
            Log.i(TAG, "cursor moveToFirst=false");
            return false;
        }

        @Override
        public String getString(int columnIndex) {
            access("getString", columnIndex);
            return null;
        }

        @Override
        public short getShort(int columnIndex) {
            access("getShort", columnIndex);
            return 0;
        }

        @Override
        public int getInt(int columnIndex) {
            access("getInt", columnIndex);
            return 0;
        }

        @Override
        public long getLong(int columnIndex) {
            access("getLong", columnIndex);
            return 0L;
        }

        @Override
        public float getFloat(int columnIndex) {
            access("getFloat", columnIndex);
            return 0F;
        }

        @Override
        public double getDouble(int columnIndex) {
            access("getDouble", columnIndex);
            return 0D;
        }

        @Override
        public byte[] getBlob(int columnIndex) {
            access("getBlob", columnIndex);
            return null;
        }

        @Override
        public int getType(int columnIndex) {
            access("getType", columnIndex);
            return Cursor.FIELD_TYPE_NULL;
        }

        @Override
        public boolean isNull(int columnIndex) {
            access("isNull", columnIndex);
            return true;
        }

        private static void access(String operation, int columnIndex) {
            Log.i(TAG, "cursor access=" + operation + " index=" + columnIndex);
        }
    }
    // //// /记录 Cursor 列和访问顺序但不返回字段值 ////

    // //// 限制日志文本长度并移除换行 [@x380kkm 2026-08-03] ////
    private static String safeText(String value) {
        if (value == null) {
            return "<null>";
        }
        String compact = value.replace('\r', ' ').replace('\n', ' ');
        if (compact.length() > 96) {
            return compact.substring(0, 96);
        }
        return compact;
    }
    // //// /限制日志文本长度并移除换行 ////

    // //// 读取 URI authority 而不记录路径或查询参数 [@x380kkm 2026-07-28] ////
    private static String authority(Uri uri) {
        if (uri == null || uri.getAuthority() == null) {
            return "<none>";
        }
        return uri.getAuthority();
    }
    // //// /读取 URI authority 而不记录路径或查询参数 ////

    // //// 读取数组长度而不读取数组内容 [@x380kkm 2026-07-28] ////
    private static int arraySize(String[] values) {
        if (values == null) {
            return 0;
        }
        return values.length;
    }
    // //// /读取数组长度而不读取数组内容 ////

}
